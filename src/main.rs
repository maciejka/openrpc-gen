use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

mod command_line;
mod config;
mod deps;
mod fix;
mod gen;
mod parse;

fn main() -> ExitCode {
    let cmd = command_line::from_env();
    let config = match config::load(&cmd.config) {
        Ok(config) => config,
        Err(err) => {
            let _ = print_error(format_args!("`{}`: {}", cmd.config.display(), err));
            return ExitCode::FAILURE;
        }
    };
    let document = match load_document(&cmd.document) {
        Ok(document) => document,
        Err(err) => {
            let _ = print_error(format_args!("`{}`: {}", cmd.document.display(), err));
            return ExitCode::FAILURE;
        }
    };
    let mut document = match parse::parse(&document) {
        Ok(document) => document,
        Err(errs) => {
            for err in errs {
                let _ = print_error(format_args!("`{}`: {}", err.path, err.message));
            }
            return ExitCode::FAILURE;
        }
    };
    match fix::fix(&mut document, &config) {
        Ok(_) => {}
        Err(errs) => {
            for err in errs {
                let _ = print_error(format_args!("{}", err));
            }
            return ExitCode::FAILURE;
        }
    }
    let mut output = match std::fs::File::create(&cmd.output) {
        Ok(output) => std::io::BufWriter::new(output),
        Err(err) => {
            let _ = print_error(format_args!("`{}`: {}", cmd.output.display(), err));
            return ExitCode::FAILURE;
        }
    };
    match gen::gen(&mut output, &document, &config) {
        Ok(_) => {}
        Err(err) => {
            let _ = print_error(format_args!("{}", err));
            return ExitCode::FAILURE;
        }
    }
    drop(output);
    if config.run_rustfmt {
        if let Err(err) = run_rustmft(&cmd.output) {
            let _ = print_error(format_args!("{}", err));
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Print an error message to the standard error stream.
fn print_error(args: std::fmt::Arguments) -> std::io::Result<()> {
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();

    stderr.write_all(b"\x1B[31merror\x1B[0m: ")?;
    stderr.write_fmt(args)?;
    stderr.write_all(b"\n")?;
    stderr.flush()?;

    Ok(())
}

/// Loads the document from the provided path.
fn load_document(path: &Path) -> Result<open_rpc::OpenRpc, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let buf = std::io::BufReader::new(file);
    let document = serde_json::from_reader(buf).map_err(|e| e.to_string())?;
    Ok(document)
}

/// Runs `rustfmt` on the provided path.
fn run_rustmft(path: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("rustfmt")
        .arg(path)
        .status()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "rustfmt failed",
        ));
    }
    Ok(())
}
