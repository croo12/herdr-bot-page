// Stand-in for the external herdr process. A prompt invokes the real native CLI
// inside a simulated pane, just as an agent posts its answer on Windows.
use std::{env,path::PathBuf,process::Command};
fn main(){
    let args:Vec<String>=env::args().skip(3).collect();
    let agent=r#"{"pane_id":"w1:p1","name":"tester","agent":"pi","agent_status":"idle","cwd":"."}"#;
    match (args.first().map(String::as_str),args.get(1).map(String::as_str)){
        (Some("session"),Some("list"))=>println!(r#"{{"sessions":[{{"name":"native-test","running":true}}]}}"#),
        (Some("agent"),Some("list"))=>println!(r#"{{"result":{{"agents":[{agent}]}}}}"#),
        (Some("agent"),Some("get"))|(Some("agent"),Some("start"))=>println!(r#"{{"result":{{"agent":{agent}}}}}"#),
        (Some("agent"),Some("prompt"))=>{
            let prompt=&args[3];
            let target=prompt.split_once("in herdr-bot chat ").unwrap().1.split('.').next().unwrap();
            let cli=PathBuf::from(env::var_os("HERDR_BOT_HOME").unwrap()).join("bin").join(format!("herdr-bot{}",env::consts::EXE_SUFFIX));
            let mut invoke=Command::new(cli);
            invoke.env("HERDR_PANE_ID","w1:p1");
            if prompt.contains("native-reply") && target.starts_with("room-"){invoke.args(["pass",target]);}
            else{invoke.args(["say",target,"native-reply 안녕하세요"]);}
            let out=invoke.output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            println!(r#"{{"result":{{"agent":{agent}}}}}"#)
        }
        _=>println!(r#"{{"result":{{}}}}"#)
    }
}
