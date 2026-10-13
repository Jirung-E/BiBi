// A real child process, with no network or model. Records the interactive launch contract.
use std::{env,fs,thread,time::Duration};
fn main(){
 let args=env::args().skip(1).collect::<Vec<_>>();
 let output=args.windows(2).find(|a|a[0]=="--settings").unwrap()[1].clone();
 let mut lines=vec![env::current_dir().unwrap().to_string_lossy().to_string(),env::var("CLAUDE_CONFIG_DIR").or_else(|_|env::var("CODEX_HOME")).unwrap_or_default()];
 lines.extend(args);fs::write(output,lines.join("\n")).unwrap();thread::sleep(Duration::from_millis(200));std::process::exit(7);
}
