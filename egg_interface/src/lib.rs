use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::slice;
use std::time::Duration;
use std::time::Instant;
use egg::*;
use ordered_float::NotNan;
use num_traits::Pow;

pub type EGraph = egg::EGraph<ModelicaExpr, ConstantFold>;
pub type RuleSet = Vec<egg::Rewrite<ModelicaExpr, ConstantFold>>;
pub type Runner = egg::Runner::<ModelicaExpr, ConstantFold, ()>;
pub type Constant = NotNan<f64>;

define_language! {
    pub enum ModelicaExpr {
        Symbol(Symbol),
        Constant(Constant),
        "+" = Add([Id; 2]),
        "-" = Sub([Id; 2]),
        "*" = Mul([Id; 2]),
        "/" = Div([Id; 2]),
        "^" = Pow([Id; 2]),
        "der" = Der(Id),
        "sin" = Sin(Id),
    }
}

#[derive(Default)]
pub struct ConstantFold;
impl Analysis<ModelicaExpr> for ConstantFold {
    type Data = Option<Constant>;

    fn merge(&mut self, to: &mut Self::Data, from: Self::Data) -> DidMerge {
        egg::merge_max(to, from)
    }

    fn make(egraph: &EGraph, enode: &ModelicaExpr) -> Self::Data {
        let x = |i: &Id| egraph[*i].data;
        Some(match enode {
            ModelicaExpr::Constant(n) => *n,
            ModelicaExpr::Add([a, b]) => x(a)? + x(b)?,
            ModelicaExpr::Sub([a, b]) => x(a)? - x(b)?,
            ModelicaExpr::Mul([a, b]) => x(a)? * x(b)?,
            ModelicaExpr::Div([a, b]) if x(b) != Some(NotNan::new(0.0).unwrap()) => x(a)? / x(b)?,
            ModelicaExpr::Pow([a, b]) => Pow::pow(x(a)?, x(b)?),
            _ => return None,
        })
    }

    fn modify(egraph: &mut EGraph, id: Id) {
        if let Some(i) = egraph[id].data {
            let added = egraph.add(ModelicaExpr::Constant(i));
            egraph.union(id, added);
            // to not prune, comment this out
            //egraph[id].nodes.retain(|n| n.is_leaf());
        }
    }
}

/* OMC INTERFACE */

/// make the vector of rewrite rules
#[no_mangle]
pub extern "C" fn egg_make_rules() -> Box<RuleSet> {
    let now = Instant::now();
    let rules: RuleSet = vec![
        rewrite!("add-commute";   "(+ ?a ?b)" => "(+ ?b ?a)"),
        rewrite!("add-associate"; "(+ (+ ?a ?b) ?c)" => "(+ ?a (+ ?b ?c))"),
        rewrite!("add-neutral";   "(+ ?a 0)" => "?a"),
        rewrite!("add-inverse";   "(- ?a ?a)" => "0"),

        rewrite!("sub-associate"; "(+ ?a (- ?b ?c))" => "(- (+ ?a ?b) ?c)"),

        rewrite!("mul-commute"; "(* ?a ?b)" => "(* ?b ?a)"),
        rewrite!("mul-associate"; "(* (* ?a ?b) ?c)" => "(* ?a (* ?b ?c))"),
        rewrite!("mul-1"; "(* ?a 1)" => "?a"),

        rewrite!("div-associate"; "(* (/ ?a ?b) ?c)" => "(* ?a (/ ?c ?b))"),
        rewrite!("div-inv"; "(/ ?a ?a)" => "1"),

        rewrite!("add-mul-distribute"; "(+ (* ?a ?b) (* ?a ?c))" => "(* ?a (+ ?b ?c))"),
        rewrite!("mul-0"; "(* ?a 0)" => "0"),

        rewrite!("add-same-base"; "(+ ?a ?a)" => "(* ?a 2)"),
        rewrite!("add-same"; "(+ ?a (* ?a ?n))" => "(* ?a (+ ?n 1))"),

        rewrite!("mul-same-base"; "(* ?a ?a)" => "(^ ?a 2)"),
        rewrite!("mul-same"; "(* ?a (^ ?a ?n))" => "(^ ?a (+ ?n 1))"),

        rewrite!("pow-distribute"; "(^ (* ?a ?b) ?n)" => "(* (^ ?a ?n) (^ ?b ?n))"),

        rewrite!("sin-0"; "(sin 0)" => "0"),
    ];
    let elapsed = now.elapsed();
    println!("made rules: {:.2?}", elapsed);
    Box::new(rules)
}

#[no_mangle]
pub unsafe extern "C" fn egg_free_rules(_rules: Option<Box<RuleSet>>) {
    // dropped implicitly
    println!("dropped rules");
}

/// make the runner
#[no_mangle]
pub extern "C" fn egg_make_runner() -> Box<Runner> {
  let now = Instant::now();
  let runner = Runner::default()
    // we can load a saturated egraph here
    .with_time_limit(Duration::from_millis(500));
  let elapsed = now.elapsed();
  println!("made runner: {:.2?}", elapsed);
  Box::new(runner)
}

#[no_mangle]
pub unsafe extern "C" fn egg_free_runner(_runner: Option<Box<Runner>>) {
    // dropped implicitly
    println!("dropped runner");
}

#[no_mangle]
pub extern "C" fn egg_simplify_expr(rules: Option<&RuleSet>, runner: Option<&mut Runner>, expr_str: *const c_char) -> *mut c_char {
    let mut times = Vec::new();

    // parse the expression, the type annotation tells it which Language to use
    let now = Instant::now();
    let expr = unsafe { CStr::from_ptr(expr_str).to_string_lossy().into_owned() };
    let expr: RecExpr<ModelicaExpr> = expr.parse().unwrap();
    times.push((now.elapsed(), "expr     "));

    let now = Instant::now();
    let cost = AstSize.cost_rec(&expr);
    times.push((now.elapsed(), "cost     "));

    // simplify the expression using a Runner, which creates an e-graph with
    // the given expression and runs the given rules over it
    //println!("{:?}", runner0);
    let now = Instant::now();
    let rules = rules.unwrap();
    let runner = runner.unwrap();
    //let egraph = runner.egraph.copy_without_unions(ConstantFold);
    let runner = Runner::default()
    //    .with_egraph(egraph)
        .with_time_limit(Duration::from_millis(100))
        .with_iter_limit(10)
        .with_node_limit(1000)
        .with_expr(&expr).run(rules);
    times.push((now.elapsed(), "runner   "));
    println!("{:?}", runner.stop_reason);
    //println!("{:?}", runner);

    // the Runner knows which e-class the expression given with `with_expr` is in
    let now = Instant::now();
    let root = runner.roots[0];
    times.push((now.elapsed(), "root     "));

    // use an Extractor to pick the best element of the root eclass
    let now = Instant::now();
    let extractor = Extractor::new(&runner.egraph, AstSize);
    times.push((now.elapsed(), "extractor"));

    let now = Instant::now();
    let (best_cost, best) = extractor.find_best(root);
    times.push((now.elapsed(), "best     "));

    println!("cost {} -> {}", cost, best_cost);
    //println!("expr {}\n  -> {}", expr, best);
    times.sort_by(|(a,_), (b,_)| b.cmp(a));
    print!("{}", times.iter().fold(String::new(), |acc, (t,s)| acc + &format!("{}\t{:.2?}", s, t) + "\n"));

    CString::new(best.to_string()).expect("return string error").into_raw()
}


/*----------------------------------------------------------------------------*/
/* useful functions between Rust and C */
/*----------------------------------------------------------------------------*/


// A Rust struct mapping the C struct
#[repr(C)]
#[derive(Debug)]
pub struct RustStruct {
    pub c: char,
    pub ul: u64,
    pub c_string: *const c_char,
}

macro_rules! create_function {
    // This macro takes an argument of designator `ident` and
    // creates a function named `$func_name`.
    // The `ident` designator is used for variable/function names.
    ($func_name:ident, $ctype:ty) => {
        #[no_mangle]
        pub extern "C" fn $func_name(v: $ctype) {
            // The `stringify!` macro converts an `ident` into a string.
            println!(
                "{:?}() is called, value passed = <{:?}>",
                stringify!($func_name),
                v
            );
        }
    };
}

// create simple functions where C type is exactly mapping a Rust type
//create_function!(rust_char, char);
//create_function!(rust_wchar, char);
create_function!(rust_short, i16);
create_function!(rust_ushort, u16);
create_function!(rust_int, i32);
create_function!(rust_uint, u32);
create_function!(rust_long, i64);
create_function!(rust_ulong, u64);
create_function!(rust_void, *mut c_void);

// for NULL-terminated C strings, it's a little bit clumsier
#[no_mangle]
pub extern "C" fn rust_string(c_string: *const c_char) {
    // build a Rust string from C string
    let s = unsafe { CStr::from_ptr(c_string).to_string_lossy().into_owned() };

    println!("rust_string() is called, value passed = <{:?}>", s);
}

// for C arrays, need to pass array size
#[no_mangle]
pub extern "C" fn rust_int_array(c_array: *const i32, length: usize) {
    // build a Rust array from array & length
    let rust_array: &[i32] = unsafe { slice::from_raw_parts(c_array, length as usize) };
    println!(
        "rust_int_array() is called, value passed = <{:?}>",
        rust_array
    );
}

#[no_mangle]
pub extern "C" fn rust_string_array(c_array: *const *const c_char, length: usize) {
    // build a Rust array from array & length
    let tmp_array: &[*const c_char] = unsafe { slice::from_raw_parts(c_array, length as usize) };

    // convert each element to a Rust string
    let rust_array: Vec<_> = tmp_array
        .iter()
        .map(|&v| unsafe { CStr::from_ptr(v).to_string_lossy().into_owned() })
        .collect();

    println!(
        "rust_string_array() is called, value passed = <{:?}>",
        rust_array
    );
}

// for C structs, need to convert each individual Rust member if necessary
#[no_mangle]
pub unsafe extern "C" fn rust_cstruct(c_struct: *mut RustStruct) {
    let rust_struct = &*c_struct;
    let s = CStr::from_ptr(rust_struct.c_string)
        .to_string_lossy()
        .into_owned();

    println!(
        "rust_cstruct() is called, value passed = <{} {} {}>",
        rust_struct.c, rust_struct.ul, s
    );
}
