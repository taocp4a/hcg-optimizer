fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: hcg-bench <input.zip> [--overview]");
    let overview = args.next().as_deref() == Some("--overview");
    let zip = std::fs::read(path).expect("read input");
    let start = std::time::Instant::now();
    let now = || start.elapsed().as_secs_f64() * 1000.0;
    if overview {
        println!("{}", hcg_bench::overview(zip, &now));
    } else {
        println!("{}", hcg_bench::pipeline(zip, &now));
    }
}
