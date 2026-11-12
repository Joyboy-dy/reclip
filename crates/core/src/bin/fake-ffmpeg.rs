//! Test double for ffmpeg: copies the `-i` input to the last argument and
//! records its argv in `<output>.argv.txt`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let input = args
        .iter()
        .position(|a| a == "-i")
        .and_then(|i| args.get(i + 1))
        .expect("-i input");
    let output = args.last().expect("output");
    std::fs::copy(input, output).expect("copy input");
    std::fs::write(format!("{output}.argv.txt"), args.join("\n")).expect("record argv");
}
