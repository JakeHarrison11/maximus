#[cfg(not(target_arch = "wasm32"))]
use maximus_app::Options;

#[cfg(not(target_arch = "wasm32"))]
const HELP: &str = "Maximus — a textbook-style frontend for Maxima

USAGE:
    maximus [OPTIONS] [NOTEBOOK]

OPTIONS:
    --minimal           Start in minimal (calculator) mode
    --mcp               Run as a Model Context Protocol server over stdio (no GUI)
    --run               Evaluate all cells of the notebook on startup
    --maxima <PATH>     Path to the maxima executable
    -V, --version       Print version
    -h, --help          Print this help
";

#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(async {
        if let Err(e) = maximus_app::start("maximus_canvas".to_string()).await {
            web_sys::console::error_1(&e);
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut opts = Options::default();
    let mut mcp = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{}", HELP);
                return;
            }
            "-V" | "--version" => {
                println!("maximus {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--minimal" => opts.minimal = true,
            "--mcp" => mcp = true,
            "--run" => opts.run_all = true,
            "--maxima" => opts.maxima_path = args.next(),
            p if !p.starts_with('-') => opts.open_file = Some(p.into()),
            other => {
                eprintln!("unknown option {}\n\n{}", other, HELP);
                std::process::exit(2);
            }
        }
    }
    if mcp {
        if let Err(e) = maximus_app::run_mcp(opts) {
            eprintln!("maximus mcp: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if let Err(e) = maximus_app::run_native(opts) {
        eprintln!("maximus: {}", e);
        std::process::exit(1);
    }
}
