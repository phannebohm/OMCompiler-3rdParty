use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::slice;
use std::time::Instant;
use egg::*;

define_language! {
    enum SimpleLanguage {
        Num(i32),
        "+" = Add([Id; 2]),
        "-" = Sub([Id; 2]),
        "*" = Mul([Id; 2]),
        "/" = Div([Id; 2]),
        "^" = Pow([Id; 2]),
        "der" = Der(Id),
        "sin" = Sin(Id),
        Symbol(Symbol),
    }
}

/// make the vector of rewrite rules
/// TODO read this from a file, read only once
fn make_rules() -> Vec<Rewrite<SimpleLanguage, ()>> {
    println!("making rules");
    vec![
        rewrite!("commute-add"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rewrite!("associate-add"; "(+ (+ ?a ?b) ?c)" => "(+ ?a (+ ?b ?c))"),

        rewrite!("associate-sub"; "(+ ?a (- ?b ?c))" => "(- (+ ?a ?b) ?c)"),

        rewrite!("commute-mul"; "(* ?a ?b)" => "(* ?b ?a)"),
        rewrite!("associate-mul"; "(* (* ?a ?b) ?c)" => "(* ?a (* ?b ?c))"),

        rewrite!("distribute"; "(+ (* ?a ?b) (* ?a ?c))" => "(* ?a (+ ?b ?c))"),

        rewrite!("add-same"; "(+ ?a ?a)" => "(* ?a 2)"),
        rewrite!("add-same3"; "(+ (+ ?a ?a) ?a)" => "(* ?a 3)"),

        rewrite!("add-0"; "(+ ?a 0)" => "?a"),
        rewrite!("sub-inv"; "(- ?a ?a)" => "0"),
        rewrite!("mul-0"; "(* ?a 0)" => "0"),
        rewrite!("mul-1"; "(* ?a 1)" => "?a"),

        //rewrite!("binomial-1"; "" => ""),

        rewrite!("sin-0"; "(sin 0)" => "0"),
    ]
}

/// parse an expression, simplify it using egg, and pretty print it back out
fn simplify(s: &str, rules: &Vec<Rewrite<SimpleLanguage, ()>>) {
    let mut times = Vec::new();

    // parse the expression, the type annotation tells it which Language to use
    let now = Instant::now();
    let expr: RecExpr<SimpleLanguage> = s.parse().unwrap();
    times.push((now.elapsed(), "expr     "));

    let now = Instant::now();
    let cost = AstSize.cost_rec(&expr);
    times.push((now.elapsed(), "cost     "));

    // simplify the expression using a Runner, which creates an e-graph with
    // the given expression and runs the given rules over it
    let now = Instant::now();
    let runner = Runner::default().with_expr(&expr).run(rules);
    times.push((now.elapsed(), "runner   "));

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

    times.sort_by(|(a,_), (b,_)| b.cmp(a));
    println!("{}", times.iter().fold(String::new(), |acc, (t,s)| acc + &format!("{}\t{:.2?}", s, t) + "\n"));
    println!("Simplified {} with cost {}\nto         {} with cost {}", expr, cost, best, best_cost);
}

#[test]
fn simple_tests() {
    assert_eq!(simplify("(* 0 42)"), "0");
    assert_eq!(simplify("(+ 0 (* 1 foo))"), "foo");
}

/* OMC INTERFACE */

#[no_mangle]
pub extern "C" fn egg_simplify_equation(lhs_str: *const c_char, rhs_str: *const c_char) {
    let lhs = unsafe { CStr::from_ptr(lhs_str).to_string_lossy().into_owned() };
    let rhs = unsafe { CStr::from_ptr(rhs_str).to_string_lossy().into_owned() };
    let now = Instant::now();
    let rules = make_rules();
    let elapsed = now.elapsed();
    println!("rules:   {:.2?}", elapsed);
    simplify(&lhs, &rules);
    simplify(&rhs, &rules);
}

#[no_mangle]
pub extern "C" fn egg_rules(data: *mut c_void) -> Vec<Rewrite<SimpleLanguage, ()>> {
    let rules = unsafe { &mut *(data as *mut Vec<Rewrite<SimpleLanguage, ()>>) };
    rules.to_vec()
}


/*---------------------------------------------------------------------------------------*/
/* useful functions between Rust and C */
/*---------------------------------------------------------------------------------------*/


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
