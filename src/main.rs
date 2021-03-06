#[forbid(unsafe_code)]
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(about, author)]
struct Opts {}

fn main() {
    let _opts: Opts = Opts::from_args();
    println!("Hello, world!");
}
