//! A Dedicated Server build runs headless and serves: lay out a server build around the real
//! player executable, start it with no arguments (as a host would), then connect a headless
//! client process to the port it reports.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use misty_engine::build::{BUILD_INFO_FILE, BuildInfo, BuildScene, BuildSettings, PAK_FILE};
use misty_engine::network::NetworkManager;
use misty_engine::scene::{Scene, ScriptRef, Scripts};

/// A project with a networked scene and a script that reports the server define.
fn project(root: &Path) {
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Hello.lua"),
        r#"
local Hello = {}
function Hello:Start()
--#if MISTY_SERVER
    Debug.Log("server define on, dedicated=" .. tostring(Application.isDedicatedServer))
--#else
    Debug.Log("client build")
--#endif
end
return Hello
"#,
    )
    .unwrap();
    let mut scene = Scene::new("Arena");
    let m = scene.spawn("Network Manager", None);
    scene.add(
        m,
        NetworkManager {
            port: 0,
            listen_address: "127.0.0.1".into(),
            lan_discovery: false,
            ..Default::default()
        },
    );
    scene.add(
        m,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Hello.lua".into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    scene.save(&root.join("Assets/Scenes/Arena.scene")).unwrap();
    BuildSettings {
        scenes: vec![BuildScene::new("Assets/Scenes/Arena.scene")],
        product_name: "Arena Server".into(),
        ..Default::default()
    }
    .save(root)
    .unwrap();
}

/// `<out>/Arena Server.exe` + `Arena Server_Data/data.pak` with a server BuildInfo.
fn server_build(project: &Path, out: &Path) -> PathBuf {
    let exe = out.join(format!("Arena Server{}", std::env::consts::EXE_SUFFIX));
    let data = out.join("Arena Server_Data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_misty_player"), &exe).unwrap();
    let file = std::fs::File::create(data.join(PAK_FILE)).unwrap();
    let mut pak = misty_engine::pak::PakWriter::new(file, true).unwrap();
    for path in misty_engine::vfs::files_under(project) {
        let rel = path.strip_prefix(project).unwrap();
        let rel = rel.to_string_lossy().replace('\\', "/");
        pak.add(&rel, &std::fs::read(&path).unwrap()).unwrap();
    }
    let info = BuildInfo {
        development: true,
        target: String::new(),
        version: "1.0".into(),
        server: true,
        profile: "Dedicated Server".into(),
        defines: vec!["MISTY_SERVER".into(), "DEVELOPMENT_BUILD".into()],
    };
    pak.add(BUILD_INFO_FILE, &serde_json::to_vec(&info).unwrap())
        .unwrap();
    pak.finish().unwrap();
    exe
}

/// Start `exe` with `args`; its stderr lines arrive on the receiver.
fn start(exe: &Path, args: &[&str]) -> (Child, Receiver<String>) {
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(exe.parent().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the player");
    let (tx, rx) = channel();
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    (child, rx)
}

/// Read lines until one satisfies `f` (all lines read are kept in `seen`).
fn wait_for(
    rx: &Receiver<String>,
    seen: &mut Vec<String>,
    secs: u64,
    f: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(l) = seen.iter().find(|l| f(l)) {
        return Some(l.clone());
    }
    let until = Instant::now() + Duration::from_secs(secs);
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(line) => {
                let hit = f(&line);
                seen.push(line.clone());
                if hit {
                    return Some(line);
                }
            }
            Err(_) => break,
        }
    }
    None
}

/// Kills the processes and removes the folders whatever happens.
struct Cleanup {
    children: Vec<Child>,
    dirs: Vec<PathBuf>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
        for d in &self.dirs {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

#[test]
fn server_build_runs_headless_and_a_headless_client_connects() {
    let base = std::env::temp_dir().join(format!("misty_server_build_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (proj, out) = (base.join("Project"), base.join("Build"));
    let mut cleanup = Cleanup {
        children: Vec::new(),
        dirs: vec![base.clone()],
    };
    project(&proj);
    let exe = server_build(&proj, &out);

    // The server: no arguments, like a host machine starting the build.
    let (server, server_rx) = start(&exe, &[]);
    cleanup.children.push(server);
    let mut server_log = Vec::new();
    let line = wait_for(&server_rx, &mut server_log, 90, |l| {
        l.contains("Server listening on port")
    })
    .unwrap_or_else(|| panic!("the server didn't start:\n{}", server_log.join("\n")));
    let port: u16 = line.rsplit(' ').next().unwrap().trim().parse().unwrap();
    assert!(port > 0);
    assert!(
        server_log
            .iter()
            .any(|l| l.contains("no window, audio or GPU")),
        "{server_log:?}"
    );
    wait_for(&server_rx, &mut server_log, 30, |l| {
        l.contains("server define on, dedicated=true")
    })
    .unwrap_or_else(|| panic!("the server script didn't run:\n{}", server_log.join("\n")));

    // A headless client process of the same build connects to it.
    let addr = format!("127.0.0.1:{port}");
    let (client, client_rx) = start(&exe, &["--headless", "--net-client", &addr]);
    cleanup.children.push(client);
    let mut client_log = Vec::new();
    wait_for(&client_rx, &mut client_log, 60, |l| {
        l.contains("connected as client")
    })
    .unwrap_or_else(|| panic!("the client didn't connect:\n{}", client_log.join("\n")));
    wait_for(&server_rx, &mut server_log, 60, |l| {
        l.contains("Network: client 1 joined")
    })
    .unwrap_or_else(|| panic!("the server saw no client:\n{}", server_log.join("\n")));
    // Nothing of a window or renderer ever started.
    let all: Vec<&String> = server_log.iter().chain(client_log.iter()).collect();
    assert!(
        !all.iter()
            .any(|l| l.contains("Renderer:") || l.contains("window:")),
        "{all:?}"
    );
    assert!(
        !all.iter().any(|l| l.starts_with("Error")),
        "errors: {all:?}"
    );
}
