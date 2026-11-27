use risp::Risp;

struct Context {}

pub fn main() {
    let mut risp = Risp::new(Context {});
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        match risp.eval_to_string(&line) {
            Ok(output) => println!("{output}"),
            Err(error) => eprintln!("{error}"),
        }
    }
}