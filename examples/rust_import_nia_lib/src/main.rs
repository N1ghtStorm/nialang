unsafe extern "C" {
    fn nia_add(a: i32, b: i32) -> i32;
    fn nia_double(x: i32) -> i32;
    fn something() -> i32;
}

fn main() {
    let sum = unsafe { nia_add(20, 22) };
    let doubled = unsafe { nia_double(21) };
    let smth = unsafe { something() };

    println!("nia_add(20, 22) = {sum}");
    println!("nia_double(21) = {doubled}");
    println!("something() = {smth}");

    assert_eq!(sum, 42);
    assert_eq!(doubled, 42);
}
