//! Diagnostika: co se dá o verzi Windows a stavu aktualizací přečíst.
fn main() {
    let i = win_sys::osinfo::os_info();
    println!("{i:#?}");
}
