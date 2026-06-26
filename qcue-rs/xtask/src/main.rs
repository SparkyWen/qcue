use xtask::lints;

fn main() {
    let root = lints::workspace_root();
    let v = lints::check_layering_law(&root);
    let p = lints::check_protocol_deps_minimal(&root);
    if v.is_empty() && p.is_empty() {
        println!("xtask lints: OK");
    } else {
        for s in v.iter().chain(p.iter()) {
            eprintln!("LINT: {s}");
        }
        std::process::exit(1);
    }
}
