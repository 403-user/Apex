#[test]
fn test_vte_parser_creation() {
    let processor = vte_core::VteProcessor::new(24, 80, 10000);
    assert_eq!(processor.grid.rows_visible, 24);
    assert_eq!(processor.grid.cols, 80);
}

#[test]
fn test_grid_resize() {
    let mut processor = vte_core::VteProcessor::new(24, 80, 10000);
    processor.grid.resize(50, 120);
    assert_eq!(processor.grid.rows_visible, 50);
    assert_eq!(processor.grid.cols, 120);
}

#[test]
fn test_session_manager() {
    let mgr = apex_server::session::SessionManager::new();
    let id = mgr.create_session("test".into());
    let session = mgr.get_session(id);
    assert!(session.is_some());
    assert_eq!(session.unwrap().name, "test");
}

#[test]
fn test_protocol_message_roundtrip() {
    let msg = apex_protocol::wire::Message::new(
        apex_protocol::wire::MessageType::Ping,
        "hello".into(),
    );
    let bytes = msg.to_bytes();
    let decoded = apex_protocol::wire::Message::from_bytes(&bytes).unwrap();
    assert_eq!(decoded.msg_type, apex_protocol::wire::MessageType::Ping);
    assert_eq!(decoded.payload, "hello");
}

#[test]
fn test_workspace_creation() {
    let ws = apex_collab::Workspace::new("engagement-01");
    assert_eq!(ws.panes.len(), 1);
    assert_eq!(ws.name, "engagement-01");
}

#[test]
fn test_command_sandbox() {
    let sandbox = apex_ai::sandbox::CommandSandbox::new();
    assert!(sandbox.validate("nmap", &[]).is_ok());
    assert!(sandbox.validate("rm", &["-rf".to_string(), "/".to_string()]).is_err());
}

#[test]
fn test_layout_splitting() {
    let id1 = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    let leaf1 = apex_mux::Layout::new_leaf(id1);
    let leaf2 = apex_mux::Layout::new_leaf(id2);
    let split = apex_mux::Layout::split_horizontal(leaf1, leaf2, 0.5);
    assert_eq!(split.pane_ids().len(), 2);
}

// ============ Phase 2: Shell Stabilization & PTY Tests ============

#[test]
fn test_pty_instance_creation() {
    let pty = apex_pty::PtyInstance::new(24, 80).unwrap();
    assert_eq!(pty.rows, 24);
    assert_eq!(pty.cols, 80);
    assert!(pty.child.is_some());
}

#[test]
fn test_pty_resize() {
    let mut pty = apex_pty::PtyInstance::new(24, 80).unwrap();
    assert!(pty.resize(50, 120).is_ok());
    assert_eq!(pty.rows, 50);
    assert_eq!(pty.cols, 120);
}

#[test]
fn test_pty_write_read() {
    let pty = apex_pty::PtyInstance::new(24, 80).unwrap();
    let written = pty.write(b"echo hello\n").unwrap();
    assert!(written > 0);
    let mut buf = [0u8; 4096];
    std::thread::sleep(std::time::Duration::from_millis(100));
    let read = pty.read(&mut buf).unwrap_or(0);
    assert!(read > 0);
    let output = String::from_utf8_lossy(&buf[..read]);
    assert!(output.contains("hello"));
}

#[test]
fn test_channel_config_defaults() {
    let cfg = apex_pty::ChannelConfig::default();
    assert_eq!(cfg.host, "0.0.0.0");
    assert_eq!(cfg.port, 4444);
    assert!(!cfg.use_ssl);
}

#[test]
fn test_stabilizer_probes() {
    let stab = apex_pty::PtyStabilizer::new();
    assert_eq!(stab.probes.len(), 6);
    assert_eq!(stab.probes[0].binary, "script");
    assert_eq!(stab.probes[1].binary, "python3");
    assert_eq!(stab.probes[2].binary, "python");
    assert!(stab.probes[0].priority < stab.probes[1].priority);
}

#[test]
fn test_stabilizer_pty_check() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.check_has_pty();
    assert!(cmd.contains("HAS_PTY") && cmd.contains("NO_PTY"));
}

#[test]
fn test_stabilizer_interactive_env() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.set_interactive_env(24, 80, "xterm-256color");
    assert!(cmd.contains("stty rows 24"));
    assert!(cmd.contains("columns 80"));
    assert!(cmd.contains("TERM='xterm-256color'"));
}

#[test]
fn test_stabilizer_noninteractive_env() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.set_noninteractive_env();
    assert!(cmd.contains("stty -echo"));
    assert!(cmd.contains("PS1="));
}

#[test]
fn test_stabilizer_probe_command() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.get_probe_command(&stab.probes[1]);
    assert!(cmd.contains("which python3"));
}

#[test]
fn test_tamper_tracker_created_file() {
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.track_created_file("/tmp/exploit.sh", "test_module", 1000);
    assert_eq!(tracker.list_active().len(), 1);
    assert_eq!(tracker.tampers.len(), 1);
}

#[test]
fn test_tamper_tracker_replaced_file() {
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.track_replaced_file("/etc/passwd", vec![0x00, 0x01, 0x02], "escalate_module", 0);
    assert_eq!(tracker.list_active().len(), 1);
}

#[test]
fn test_tamper_tracker_revert() {
    let tmp = std::env::temp_dir().join("apex-test-revert.txt");
    std::fs::write(&tmp, b"test data").unwrap();
    let path = tmp.to_str().unwrap().to_string();
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.track_created_file(&path, "test", 1000);
    let id = tracker.tampers.keys().next().unwrap().clone();
    assert!(tracker.revert_tamper(&id).is_ok());
    assert!(tracker.tampers.get(&id).unwrap().reverted);
    assert_eq!(tracker.list_active().len(), 0);
    assert!(!tmp.exists());
}

#[test]
fn test_tamper_tracker_double_revert() {
    let tmp = std::env::temp_dir().join("apex-test-double-revert.txt");
    std::fs::write(&tmp, b"test data").unwrap();
    let path = tmp.to_str().unwrap().to_string();
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.track_created_file(&path, "test", 1000);
    let id = tracker.tampers.keys().next().unwrap().clone();
    assert!(tracker.revert_tamper(&id).is_ok());
    assert!(tracker.revert_tamper(&id).is_err());
    assert!(!tmp.exists());
}

#[test]
fn test_tamper_tracker_revert_all() {
    let tmp_a = std::env::temp_dir().join("apex-test-revert-a.txt");
    let tmp_b = std::env::temp_dir().join("apex-test-revert-b.txt");
    std::fs::write(&tmp_a, b"data a").unwrap();
    std::fs::write(&tmp_b, b"data b").unwrap();
    let path_a = tmp_a.to_str().unwrap().to_string();
    let path_b = tmp_b.to_str().unwrap().to_string();
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.track_created_file(&path_a, "mod1", 1000);
    tracker.track_created_file(&path_b, "mod2", 1001);
    let results = tracker.revert_all();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|(_, ok)| *ok));
    assert_eq!(tracker.list_active().len(), 0);
    assert!(!tmp_a.exists());
    assert!(!tmp_b.exists());
}

#[test]
fn test_tamper_tracker_disabled() {
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.tracking_enabled = false;
    tracker.track_created_file("/tmp/test.txt", "test", 1000);
    assert_eq!(tracker.tampers.len(), 0);
}

#[test]
fn test_tamper_tracker_all_types() {
    let mut tracker = apex_pty::TamperTracker::new();
    tracker.track_created_file("/tmp/file.txt", "mod1", 1000);
    tracker.track_created_directory("/tmp/backup", "mod2", 1000);
    tracker.track_replaced_file("/etc/config", vec![0xff], "mod3", 1000);
    assert_eq!(tracker.list_active().len(), 3);
}

#[test]
fn test_target_environment_defaults() {
    let env = apex_pty::TargetEnvironment::default();
    assert_eq!(env.rows, 24);
    assert_eq!(env.cols, 80);
    assert_eq!(env.term, "xterm-256color");
    assert!(env.hostname.is_none());
}

#[test]
fn test_target_environment_sync_command() {
    let mut env = apex_pty::TargetEnvironment::default();
    env.rows = 50;
    env.cols = 120;
    let cmd = env.sync_command();
    assert!(cmd.contains("stty rows 50"));
    assert!(cmd.contains("columns 120"));
    assert!(cmd.contains("TERM='xterm-256color'"));
}

#[test]
fn test_target_environment_discovery_commands() {
    let env = apex_pty::TargetEnvironment::default();
    let cmds = env.discovery_commands();
    assert!(cmds.contains(&"hostname 2>/dev/null"));
    assert!(cmds.contains(&"uname -m 2>/dev/null"));
    assert!(cmds.contains(&"id -u 2>/dev/null"));
}

#[test]
fn test_shell_history_disabler() {
    let cmds = apex_pty::ShellHistoryDisabler::disable_command();
    assert!(cmds.contains(&"export HISTFILE=/dev/null".to_string()));
    assert!(cmds.contains(&"set +o history 2>/dev/null || true".to_string()));
}

#[test]
fn test_shell_history_disabler_combined() {
    let combined = apex_pty::ShellHistoryDisabler::combined_disable();
    assert!(combined.contains("HISTFILE=/dev/null"));
    assert!(combined.contains("HISTFILESIZE=0"));
    assert!(combined.contains("APEX_HISTORY_DISABLED"));
}

#[test]
fn test_executor_wrap_command() {
    let wrapped = apex_pty::executor::RemoteExecutor::wrap_command("whoami");
    assert!(wrapped.wrapped.contains("whoami"));
    assert!(wrapped.wrapped.contains("APEX_DELIM_"));
    assert!(!wrapped.start_delim.is_empty());
    assert_eq!(wrapped.start_delim, wrapped.start_delim);
}

#[test]
fn test_which_binary() {
    let cmd = apex_pty::executor::which_binary("nmap");
    assert!(cmd.contains("which nmap") || cmd.contains("command -v nmap"));
}

#[test]
fn test_stabilizer_upgrade_shell() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.upgrade_shell_command("bash");
    assert!(cmd.contains("exec bash"));
}

#[test]
fn test_stabilizer_sync_paths() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.sync_paths_command();
    assert!(cmd.contains("/usr/local/bin"));
    assert!(cmd.contains("/sbin"));
}

#[test]
fn test_stabilizer_detect_shell() {
    let stab = apex_pty::PtyStabilizer::new();
    let cmd = stab.detect_shell_command();
    assert!(cmd.contains("basename"));
}

#[test]
fn test_stabilizer_better_shells() {
    let stab = apex_pty::PtyStabilizer::new();
    assert!(stab.better_shells.contains(&"bash".to_string()));
    assert!(stab.better_shells.contains(&"zsh".to_string()));
}
