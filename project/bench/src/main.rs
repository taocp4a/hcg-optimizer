fn main() {
    let path = std::env::args().nth(1).expect("usage: hcg-bench <input.zip>");
    let zip = std::fs::read(path).expect("read input");
    let start = std::time::Instant::now();
    println!("{}", hcg_bench::pipeline(zip, &|| start.elapsed().as_secs_f64() * 1000.0));
}
