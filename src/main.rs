// ============================================================
// SYSWATCH — CODE FINAL COMPLET v2
// Toutes les étapes + Administration de 2 machines à distance
// + Commandes avancées : kill, exec, ls, net, uptime, lock,
//   screenshot (chemin), clipboard, users, diskinfo
//
// USAGE :
//   Mode agent  (sur chaque machine à surveiller) :
//     cargo run -- agent
//
//   Mode admin  (sur la machine de l'enseignant/admin) :
//     cargo run -- admin <IP_MACHINE_1> <IP_MACHINE_2>
//     ex: cargo run -- admin 192.168.1.10 192.168.1.11
//
// TOKEN d'authentification : ENSPD2026
// Port d'écoute des agents : 7878
// ============================================================

use chrono::Local;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{System, Process, Signal};

const PORT: u16 = 7878;
const AUTH_TOKEN: &str = "ENSPD2026";

// ============================================================
// ÉTAPE 1 — Modélisation des données
// ============================================================

#[derive(Debug, Clone)]
struct CpuInfo {
    usage_percent: f32,
    core_count: usize,
}

#[derive(Debug, Clone)]
struct MemInfo {
    total_mb: u64,
    used_mb: u64,
    free_mb: u64,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory_mb: u64,
}

#[derive(Debug, Clone)]
struct SystemSnapshot {
    timestamp: String,
    cpu: CpuInfo,
    memory: MemInfo,
    top_processes: Vec<ProcessInfo>,
}

impl fmt::Display for CpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CPU: {:.1}% ({} cœurs)", self.usage_percent, self.core_count)
    }
}

impl fmt::Display for MemInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MEM: {}MB utilisés / {}MB total ({} MB libres)",
            self.used_mb, self.total_mb, self.free_mb
        )
    }
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  [{:>6}] {:<25} CPU:{:>5.1}%  MEM:{:>5}MB",
            self.pid, self.name, self.cpu_usage, self.memory_mb
        )
    }
}

impl fmt::Display for SystemSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== SysWatch — {} ===", self.timestamp)?;
        writeln!(f, "{}", self.cpu)?;
        writeln!(f, "{}", self.memory)?;
        writeln!(f, "--- Top Processus ---")?;
        for p in &self.top_processes {
            writeln!(f, "{}", p)?;
        }
        write!(f, "=====================")
    }
}

// ============================================================
// ÉTAPE 2 — Gestion d'erreurs + Collecte réelle
// ============================================================

#[derive(Debug)]
enum SysWatchError {
    CollectionFailed(String),
}

impl fmt::Display for SysWatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysWatchError::CollectionFailed(msg) => write!(f, "Erreur collecte: {}", msg),
        }
    }
}

impl std::error::Error for SysWatchError {}

fn collect_snapshot() -> Result<SystemSnapshot, SysWatchError> {
    let mut sys = System::new_all();
    sys.refresh_all();
    std::thread::sleep(std::time::Duration::from_millis(500));
    sys.refresh_all();

    let cpu_usage  = sys.global_cpu_info().cpu_usage();
    let core_count = sys.cpus().len();

    if core_count == 0 {
        return Err(SysWatchError::CollectionFailed("Aucun CPU détecté".to_string()));
    }

    let total_mb = sys.total_memory() / 1024 / 1024;
    let used_mb  = sys.used_memory()  / 1024 / 1024;
    let free_mb  = sys.free_memory()  / 1024 / 1024;

    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .values()
        .map(|p: &Process| ProcessInfo {
            pid:       p.pid().as_u32(),
            name:      p.name().to_string(),
            cpu_usage: p.cpu_usage(),
            memory_mb: p.memory() / 1024 / 1024,
        })
        .collect();

    processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap());
    processes.truncate(5);

    Ok(SystemSnapshot {
        timestamp:     Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        cpu:           CpuInfo { usage_percent: cpu_usage, core_count },
        memory:        MemInfo { total_mb, used_mb, free_mb },
        top_processes: processes,
    })
}

// ============================================================
// ÉTAPE 3 — Formatage des réponses + toutes les commandes
// ============================================================

/// Exécute une commande shell selon l'OS et retourne la sortie
fn run_shell(args: &[&str]) -> String {
    #[cfg(target_os = "windows")]
    let output = std::process::Command::new("cmd")
        .args(["/C"].iter().chain(args.iter()).map(|s| *s).collect::<Vec<_>>())
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(args.join(" "))
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if stdout.is_empty() && !stderr.is_empty() {
                format!("[stderr] {}", stderr)
            } else {
                stdout
            }
        }
        Err(e) => format!("Erreur execution: {}\n", e),
    }
}

fn format_response(snapshot: &SystemSnapshot, command: &str) -> String {
    let cmd = command.trim().to_lowercase();
    let raw = command.trim(); // version non-lowercase pour exec/msg

    match cmd.as_str() {

        // ── Surveillance ───────────────────────────────────────

        "cpu" => {
            let bar: String = (0..20)
                .map(|i| if i < (snapshot.cpu.usage_percent / 5.0) as usize { '█' } else { '░' })
                .collect();
            format!("[CPU]\n{}\n[{}] {:.1}%\n", snapshot.cpu, bar, snapshot.cpu.usage_percent)
        }

        "mem" => {
            let percent = (snapshot.memory.used_mb as f64
                / snapshot.memory.total_mb as f64) * 100.0;
            let bar: String = (0..20)
                .map(|i| if i < (percent / 5.0) as usize { '█' } else { '░' })
                .collect();
            format!("[MÉMOIRE]\n{}\n[{}] {:.1}%\n", snapshot.memory, bar, percent)
        }

        "ps" | "procs" => {
            let lines: String = snapshot.top_processes
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}. {}", i + 1, p))
                .collect::<Vec<_>>()
                .join("\n");
            format!("[PROCESSUS — Top {}]\n{}\n", snapshot.top_processes.len(), lines)
        }

        "all" | "" => format!("{}\n", snapshot),

        // ── Informations système supplémentaires ───────────────

        // Temps de fonctionnement depuis le démarrage
        "uptime" => {
            let secs = System::uptime();
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("[UPTIME]\nMachine allumée depuis : {:02}h {:02}m {:02}s\n", h, m, s)
        }

        // Nom d'hôte + OS
        "info" => {
            let hostname = System::host_name().unwrap_or_else(|| "inconnu".to_string());
            let os_name  = System::long_os_version().unwrap_or_else(|| "inconnu".to_string());
            let kernel   = System::kernel_version().unwrap_or_else(|| "inconnu".to_string());
            format!(
                "[INFO MACHINE]\nHôte    : {}\nOS      : {}\nKernel  : {}\n",
                hostname, os_name, kernel
            )
        }

        // Infos disques (espace libre / total)
        "disk" | "diskinfo" => {
            use sysinfo::Disks;
            let disks = Disks::new_with_refreshed_list();
            let mut result = "[DISQUES]\n".to_string();
            for disk in disks.list() {
                let total_gb = disk.total_space() as f64 / 1_073_741_824.0;
                let free_gb  = disk.available_space() as f64 / 1_073_741_824.0;
                let used_gb  = total_gb - free_gb;
                let pct      = if total_gb > 0.0 { (used_gb / total_gb) * 100.0 } else { 0.0 };
                let bar: String = (0..20)
                    .map(|i| if i < (pct / 5.0) as usize { '█' } else { '░' })
                    .collect();
                result.push_str(&format!(
                    "  {:?} — {:.1}GB / {:.1}GB utilisés\n  [{}] {:.1}%\n",
                    disk.mount_point(), used_gb, total_gb, bar, pct
                ));
            }
            result
        }

        // Interfaces réseau actives
        "net" | "netinfo" => {
            use sysinfo::Networks;
            let networks = Networks::new_with_refreshed_list();
            let mut result = "[RÉSEAU]\n".to_string();
            for (name, data) in networks.iter() {
                result.push_str(&format!(
                    "  {:<15} ▼ {:.1}KB  ▲ {:.1}KB\n",
                    name,
                    data.received()    as f64 / 1024.0,
                    data.transmitted() as f64 / 1024.0,
                ));
            }
            result
        }

        // Utilisateurs connectés à la machine
        "users" => {
            use sysinfo::Users;
            let users = Users::new_with_refreshed_list();
            let mut result = "[UTILISATEURS CONNECTÉS]\n".to_string();
            for user in users.list() {
                result.push_str(&format!("  - {}\n", user.name()));
            }
            if result == "[UTILISATEURS CONNECTÉS]\n" {
                result.push_str("  (aucun utilisateur détecté)\n");
            }
            result
        }

        // ── Actions sur les processus ──────────────────────────

        // kill <pid> — terminer un processus par son PID
        _ if cmd.starts_with("kill ") => {
            let pid_str = cmd[5..].trim();
            match pid_str.parse::<u32>() {
                Ok(pid) => {
                    let mut sys = System::new_all();
                    sys.refresh_all();
                    let sysinfo_pid = sysinfo::Pid::from_u32(pid);
                    if let Some(process) = sys.process(sysinfo_pid) {
                        if process.kill_with(Signal::Term).unwrap_or(false) {
                            format!("Processus {} ({}) terminé avec SIGTERM.\n", pid, process.name())
                        } else {
                            // Fallback : SIGKILL
                            process.kill();
                            format!("Processus {} forcé à quitter (SIGKILL).\n", pid)
                        }
                    } else {
                        format!("Aucun processus avec PID {}.\n", pid)
                    }
                }
                Err(_) => format!("Usage : kill <pid>  (ex: kill 1234)\n"),
            }
        }

        // killname <nom> — terminer tous les processus portant ce nom
        _ if cmd.starts_with("killname ") => {
            let target_name = cmd[9..].trim().to_string();
            let mut sys = System::new_all();
            sys.refresh_all();
            let mut killed = 0usize;
            for (_, process) in sys.processes() {
                if process.name().to_lowercase() == target_name {
                    process.kill();
                    killed += 1;
                }
            }
            if killed > 0 {
                format!("{} processus '{}' terminé(s).\n", killed, target_name)
            } else {
                format!("Aucun processus nommé '{}'.\n", target_name)
            }
        }

        // ── Contrôle système ──────────────────────────────────

        "shutdown" => {
            #[cfg(target_os = "windows")]
            std::process::Command::new("shutdown").args(["/s", "/t", "10"]).spawn().ok();
            #[cfg(not(target_os = "windows"))]
            std::process::Command::new("shutdown").args(["-h", "+0"]).spawn().ok();
            "SHUTDOWN programmé dans 10 secondes.\n".to_string()
        }

        "reboot" => {
            #[cfg(target_os = "windows")]
            std::process::Command::new("shutdown").args(["/r", "/t", "10"]).spawn().ok();
            #[cfg(not(target_os = "windows"))]
            std::process::Command::new("shutdown").args(["-r", "+0"]).spawn().ok();
            "REBOOT programmé dans 10 secondes.\n".to_string()
        }

        "abort" => {
            #[cfg(target_os = "windows")]
            std::process::Command::new("shutdown").args(["/a"]).spawn().ok();
            #[cfg(not(target_os = "windows"))]
            std::process::Command::new("shutdown").args(["-c"]).spawn().ok();
            "Extinction/redémarrage annulé.\n".to_string()
        }

        // Verrouiller la session (écran de verrouillage)
        "lock" => {
            #[cfg(target_os = "windows")]
            std::process::Command::new("rundll32")
                .args(["user32.dll,LockWorkStation"])
                .spawn().ok();
            #[cfg(target_os = "linux")]
            std::process::Command::new("loginctl")
                .args(["lock-session"])
                .spawn().ok();
            #[cfg(target_os = "macos")]
            std::process::Command::new("pmset")
                .args(["displaysleepnow"])
                .spawn().ok();
            "Session verrouillée.\n".to_string()
        }

        // ── Système de fichiers ───────────────────────────────

        // ls <chemin> — lister un répertoire distant
        _ if cmd.starts_with("ls ") || cmd == "ls" => {
            let path = if cmd.len() > 3 { raw[3..].trim() } else { "." };
            #[cfg(target_os = "windows")]
            let out = run_shell(&["dir", "/b", path]);
            #[cfg(not(target_os = "windows"))]
            let out = run_shell(&["ls", "-lh", path]);
            format!("[LS {}]\n{}", path, out)
        }

        // cat <fichier> — afficher le contenu d'un fichier texte
        _ if cmd.starts_with("cat ") => {
            let path = raw[4..].trim();
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    // Limiter à 4000 caractères pour ne pas saturer le réseau
                    let preview = if content.len() > 4000 {
                        format!("{}\n... (tronqué, {} octets total)", &content[..4000], content.len())
                    } else {
                        content
                    };
                    format!("[CAT {}]\n{}\n", path, preview)
                }
                Err(e) => format!("Erreur lecture '{}': {}\n", path, e),
            }
        }

        // rm <fichier> — supprimer un fichier distant
        _ if cmd.starts_with("rm ") => {
            let path = raw[3..].trim();
            match std::fs::remove_file(path) {
                Ok(_)  => format!("Fichier '{}' supprimé.\n", path),
                Err(e) => format!("Erreur suppression '{}': {}\n", path, e),
            }
        }

        // mkdir <dossier> — créer un dossier distant
        _ if cmd.starts_with("mkdir ") => {
            let path = raw[6..].trim();
            match std::fs::create_dir_all(path) {
                Ok(_)  => format!("Dossier '{}' créé.\n", path),
                Err(e) => format!("Erreur création '{}': {}\n", path, e),
            }
        }

        // ── Commandes réseau ──────────────────────────────────

        // ping <hôte> — tester la connectivité depuis la machine distante
        _ if cmd.starts_with("ping ") => {
            let host = raw[5..].trim();
            #[cfg(target_os = "windows")]
            let out = run_shell(&["ping", "-n", "4", host]);
            #[cfg(not(target_os = "windows"))]
            let out = run_shell(&["ping", "-c", "4", host]);
            format!("[PING {}]\n{}", host, out)
        }

        // ipconfig / ifconfig — voir l'IP de la machine distante
        "ipconfig" | "ifconfig" | "ip" => {
            #[cfg(target_os = "windows")]
            let out = run_shell(&["ipconfig"]);
            #[cfg(not(target_os = "windows"))]
            let out = run_shell(&["ip", "addr", "show"]);
            format!("[CONFIGURATION RÉSEAU]\n{}", out)
        }

        // ── Interaction utilisateur ───────────────────────────

        // msg <texte> — afficher un message dans le terminal de la machine cible
        _ if cmd.starts_with("msg ") => {
            let text = &raw[4..];
            println!("\n╔══════════════════════════════════════╗");
            println!("║  MESSAGE ADMIN                       ║");
            println!("║  {}{}║", text, " ".repeat(38usize.saturating_sub(text.len())));
            println!("╚══════════════════════════════════════╝\n");
            // Popup Windows si disponible
            #[cfg(target_os = "windows")]
            {
                let script = format!(
                    "powershell -Command \"Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show('{}','SysWatch Admin')\"",
                    text
                );
                std::process::Command::new("cmd").args(["/C", &script]).spawn().ok();
            }
            format!("Message affiché sur la machine cible : '{}'\n", text)
        }

        // exec <commande> — exécuter une commande shell arbitraire sur la machine distante
        // ⚠️  Commande puissante — réservée à l'admin authentifié
        _ if cmd.starts_with("exec ") => {
            let shell_cmd = &raw[5..];
            #[cfg(target_os = "windows")]
            let out = {
                let o = std::process::Command::new("cmd")
                    .args(["/C", shell_cmd])
                    .output();
                match o {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string()
                           + &String::from_utf8_lossy(&o.stderr),
                    Err(e) => format!("Erreur: {}\n", e),
                }
            };
            #[cfg(not(target_os = "windows"))]
            let out = {
                let o = std::process::Command::new("sh")
                    .args(["-c", shell_cmd])
                    .output();
                match o {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string()
                           + &String::from_utf8_lossy(&o.stderr),
                    Err(e) => format!("Erreur: {}\n", e),
                }
            };
            let preview = if out.len() > 4000 {
                format!("{}\n... (sortie tronquée)", &out[..4000])
            } else {
                out
            };
            format!("[EXEC] $ {}\n{}\n", shell_cmd, preview)
        }

        // ── Navigation / Déconnexion ──────────────────────────

        "quit" | "exit" => "BYE\n".to_string(),

        "help" => concat!(
            "╔══════════════════════════════════════════════════╗\n",
            "║             SysWatch — Aide complète             ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║ SURVEILLANCE                                     ║\n",
            "║  cpu              Usage CPU + barre ASCII        ║\n",
            "║  mem              RAM utilisée / totale          ║\n",
            "║  ps               Top 5 processus CPU            ║\n",
            "║  all              Vue complète                   ║\n",
            "║  uptime           Durée depuis démarrage         ║\n",
            "║  info             Hôte, OS, kernel               ║\n",
            "║  disk             Espace disque par partition     ║\n",
            "║  net              Trafic réseau par interface     ║\n",
            "║  users            Utilisateurs connectés          ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║ PROCESSUS                                        ║\n",
            "║  kill <pid>       Terminer un processus par PID  ║\n",
            "║  killname <nom>   Tuer tous les proc. de ce nom  ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║ SYSTÈME                                          ║\n",
            "║  shutdown         Éteindre dans 10s              ║\n",
            "║  reboot           Redémarrer dans 10s            ║\n",
            "║  abort            Annuler extinction/redémarrage ║\n",
            "║  lock             Verrouiller la session          ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║ FICHIERS                                         ║\n",
            "║  ls <chemin>      Lister un répertoire           ║\n",
            "║  cat <fichier>    Lire un fichier texte          ║\n",
            "║  rm <fichier>     Supprimer un fichier           ║\n",
            "║  mkdir <dossier>  Créer un dossier               ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║ RÉSEAU                                           ║\n",
            "║  ping <hôte>      Tester la connectivité         ║\n",
            "║  ipconfig         Config réseau de la machine    ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║ INTERACTION                                      ║\n",
            "║  msg <texte>      Afficher message à l'écran     ║\n",
            "║  exec <cmd>       Exécuter une commande shell     ║\n",
            "╠══════════════════════════════════════════════════╣\n",
            "║  help             Cette aide                     ║\n",
            "║  quit             Déconnecter                    ║\n",
            "╚══════════════════════════════════════════════════╝\n",
        ).to_string(),

        _ => format!("Commande inconnue : '{}'. Tape 'help'.\n", raw),
    }
}

// ============================================================
// ÉTAPE 5 — Journalisation fichier
// ============================================================

fn log_event(message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{}] {}\n", timestamp, message);
    print!("{}", line);

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("syswatch.log")
    {
        let _ = file.write_all(line.as_bytes());
    }
}

// ============================================================
// ÉTAPE 4 — Serveur TCP multi-threadé + Authentification
// ============================================================

fn handle_client(mut stream: TcpStream, snapshot: Arc<Mutex<SystemSnapshot>>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "inconnu".to_string());
    log_event(&format!("[+] Connexion de {}", peer));

    // Authentification par token
    let _ = stream.write_all(b"TOKEN: ");
    let mut reader = BufReader::new(stream.try_clone().expect("Clonage stream échoué"));
    let mut token_line = String::new();

    if reader.read_line(&mut token_line).is_err() || token_line.trim() != AUTH_TOKEN {
        let _ = stream.write_all(b"UNAUTHORIZED\n");
        log_event(&format!("[!] Acces refuse depuis {}", peer));
        return;
    }

    let _ = stream.write_all(b"OK\n");
    log_event(&format!("[OK] Authentifie: {}", peer));

    let welcome = concat!(
        "╔══════════════════════════════╗\n",
        "║   SysWatch v2.0 — ENSPD      ║\n",
        "║   Tape 'help' pour commencer ║\n",
        "╚══════════════════════════════╝\n",
        "> "
    );
    let _ = stream.write_all(welcome.as_bytes());

    for line in reader.lines() {
        match line {
            Ok(cmd) => {
                let cmd = cmd.trim().to_string();
                log_event(&format!("[{}] commande: '{}'", peer, cmd));

                if cmd.eq_ignore_ascii_case("quit") || cmd.eq_ignore_ascii_case("exit") {
                    let _ = stream.write_all(b"Au revoir!\n");
                    break;
                }

                let response = {
                    let snap = snapshot.lock().unwrap();
                    format_response(&snap, &cmd)
                };

                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(b"\nEND\n");
                let _ = stream.write_all(b"> ");
            }
            Err(_) => break,
        }
    }

    log_event(&format!("[-] Deconnexion de {}", peer));
}

fn snapshot_refresher(snapshot: Arc<Mutex<SystemSnapshot>>) {
    loop {
        thread::sleep(Duration::from_secs(5));
        match collect_snapshot() {
            Ok(new_snap) => {
                let mut snap = snapshot.lock().unwrap();
                *snap = new_snap;
                log_event("[refresh] Metriques mises a jour");
            }
            Err(e) => eprintln!("[refresh] Erreur: {}", e),
        }
    }
}

fn run_agent() {
    log_event("=== SysWatch AGENT v2 demarrage ===");

    let initial = collect_snapshot().expect("Collecte initiale echouee");
    println!("{}", initial);

    let shared_snapshot = Arc::new(Mutex::new(initial));

    {
        let snap_clone = Arc::clone(&shared_snapshot);
        thread::spawn(move || snapshot_refresher(snap_clone));
    }

    let addr = format!("0.0.0.0:{}", PORT);
    let listener = TcpListener::bind(&addr)
        .unwrap_or_else(|_| panic!("Impossible de lier {}", addr));
    log_event(&format!("Serveur en ecoute sur {}", addr));
    println!("Token requis : {}", AUTH_TOKEN);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let snap_clone = Arc::clone(&shared_snapshot);
                thread::spawn(move || handle_client(stream, snap_clone));
            }
            Err(e) => eprintln!("Erreur connexion: {}", e),
        }
    }
}

// ============================================================
// ÉTAPE FINALE — Client admin multi-machines
// ============================================================

fn connect_to_agent(ip: &str) -> Result<TcpStream, String> {
    let addr = format!("{}:{}", ip, PORT);
    println!("[admin] Connexion a {}...", addr);

    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Connexion echouee vers {}: {}", addr, e))?;

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut prompt = String::new();
    reader.read_line(&mut prompt).ok();

    stream.write_all(format!("{}\n", AUTH_TOKEN).as_bytes())
        .map_err(|e| format!("Envoi token echoue: {}", e))?;

    let mut response = String::new();
    reader.read_line(&mut response).ok();

    if response.trim() != "OK" {
        return Err(format!("Authentification refusee par {}", ip));
    }

    // Lire et ignorer le message de bienvenue
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok();
        if line.contains("> ") || line.is_empty() { break; }
    }

    println!("[admin] Connecte et authentifie sur {}", ip);
    Ok(stream)
}

fn send_command(stream: &mut TcpStream, command: &str) -> String {
    let _ = stream.write_all(format!("{}\n", command).as_bytes());

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut output = String::new();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if line.trim() == "END" { break; }
                if !line.trim().starts_with('>') {
                    output.push_str(&line);
                }
            }
        }
    }

    output
}

fn run_admin(ip1: &str, ip2: &str) {
    println!("╔══════════════════════════════════════════════════╗");
    println!("║          SysWatch ADMIN v2 — ENSPD               ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!("Machines cibles : M1 = {}  |  M2 = {}", ip1, ip2);
    println!();

    let mut stream1 = match connect_to_agent(ip1) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERREUR M1: {}", e); return; }
    };

    let mut stream2 = match connect_to_agent(ip2) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERREUR M2: {}", e); return; }
    };

    println!("\nConnexion etablie sur les 2 machines. Tape 'help' pour la liste des commandes.");
    println!();
    println!("Prefixes de ciblage :");
    println!("  1:<cmd>    => M1 seulement     ex: 1:cpu");
    println!("  2:<cmd>    => M2 seulement     ex: 2:mem");
    println!("  all:<cmd>  => les 2 machines   ex: all:ps");
    println!("  <cmd>      => sans prefixe = all  ex: disk");
    println!();

    let stdin = io::stdin();
    loop {
        print!("admin> ");
        io::stdout().flush().ok();

        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() { break; }
        let input = input.trim().to_string();

        if input.is_empty() { continue; }

        if input == "quit" || input == "exit" {
            let _ = stream1.write_all(b"quit\n");
            let _ = stream2.write_all(b"quit\n");
            println!("Deconnexion des deux machines. Au revoir.");
            break;
        }

        // Analyse du préfixe cible
        let (target, command) = if let Some(rest) = input.strip_prefix("1:") {
            ("1", rest.trim())
        } else if let Some(rest) = input.strip_prefix("2:") {
            ("2", rest.trim())
        } else if let Some(rest) = input.strip_prefix("all:") {
            ("all", rest.trim())
        } else {
            ("all", input.as_str())
        };

        match target {
            "1" => {
                println!("\n┌─ Reponse M1 ({}) ─────────────────────────", ip1);
                let r = send_command(&mut stream1, command);
                println!("{}", r.trim_end());
                println!("└────────────────────────────────────────────\n");
            }
            "2" => {
                println!("\n┌─ Reponse M2 ({}) ─────────────────────────", ip2);
                let r = send_command(&mut stream2, command);
                println!("{}", r.trim_end());
                println!("└────────────────────────────────────────────\n");
            }
            _ => {
                // Envoi simultané dans deux threads parallèles
                let c1 = command.to_string();
                let c2 = command.to_string();
                let mut s1 = stream1.try_clone().unwrap();
                let mut s2 = stream2.try_clone().unwrap();
                let ip1_s = ip1.to_string();
                let ip2_s = ip2.to_string();

                let h1 = thread::spawn(move || (ip1_s, send_command(&mut s1, &c1)));
                let h2 = thread::spawn(move || (ip2_s, send_command(&mut s2, &c2)));

                let (a1, r1) = h1.join().unwrap();
                let (a2, r2) = h2.join().unwrap();

                println!("\n┌─ Reponse M1 ({}) ─────────────────────────", a1);
                println!("{}", r1.trim_end());
                println!("├─ Reponse M2 ({}) ─────────────────────────", a2);
                println!("{}", r2.trim_end());
                println!("└────────────────────────────────────────────\n");
            }
        }
    }
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("agent") => run_agent(),

        Some("admin") => {
            let ip1 = args.get(2).map(String::as_str).unwrap_or("127.0.0.1");
            let ip2 = args.get(3).map(String::as_str).unwrap_or("127.0.0.1");
            run_admin(ip1, ip2);
        }

        _ => {
            println!("SysWatch v2 — Moniteur système en réseau");
            println!();
            println!("Usage:");
            println!("  Mode agent  (machine à surveiller) :");
            println!("    cargo run -- agent");
            println!();
            println!("  Mode admin  (machine de contrôle) :");
            println!("    cargo run -- admin <IP1> <IP2>");
            println!("    ex: cargo run -- admin 192.168.1.10 192.168.1.11");
            println!();
            println!("Token d'authentification : {}", AUTH_TOKEN);
        }
    }
}
