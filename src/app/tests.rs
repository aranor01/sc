use super::*;
use super::commands::resolve_default_action_template;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}
fn rp(
    dir1: Option<&str>,
    dir2: Option<&str>,
    flag: Option<bool>,
    config: bool,
    saved: Option<(&str, &str)>,
    cwd: &str,
) -> StartupPaths {
    resolve_startup_paths(
        dir1.map(Path::new),
        dir2.map(Path::new),
        flag,
        config,
        saved.map(|(l, r)| (Path::new(l), Path::new(r))),
        Path::new(cwd),
    )
}

#[test]
fn no_args_config_false() {
    let r = rp(None, None, None, false, Some(("/saved/left", "/saved/right")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/cwd"), right: p("/cwd") });
}

#[test]
fn no_args_config_true() {
    let r = rp(None, None, None, true, Some(("/saved/left", "/saved/right")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/saved/left"), right: p("/saved/right") });
}

#[test]
fn no_args_restore_flag_overrides_config() {
    let r = rp(None, None, Some(true), false, Some(("/s/l", "/s/r")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/s/l"), right: p("/s/r") });
}

#[test]
fn no_args_no_restore_flag_overrides_config() {
    let r = rp(None, None, Some(false), true, Some(("/s/l", "/s/r")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/cwd"), right: p("/cwd") });
}

#[test]
fn dir1_only_sets_both_panels() {
    let r = rp(Some("/tmp"), None, None, false, None, "/cwd");
    assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/tmp") });
}

#[test]
fn dir1_only_flag_ignored() {
    let r = rp(Some("/tmp"), None, Some(true), false, Some(("/s/l", "/s/r")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/tmp") });
}

#[test]
fn dir1_and_dir2() {
    let r = rp(Some("/tmp"), Some("/var"), None, false, None, "/cwd");
    assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/var") });
}

#[test]
fn dir1_and_dir2_flag_ignored() {
    let r = rp(Some("/tmp"), Some("/var"), Some(true), false, Some(("/s/l", "/s/r")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/var") });
}

#[test]
fn restore_with_no_saved_state_falls_back_to_cwd() {
    let r = rp(None, None, Some(true), true, None, "/cwd");
    assert_eq!(r, StartupPaths { left: p("/cwd"), right: p("/cwd") });
}

#[test]
fn dir1_relative_is_absolutized_against_cwd() {
    let r = rp(Some("subdir"), None, None, false, None, "/cwd");
    assert_eq!(r, StartupPaths { left: p("/cwd/subdir"), right: p("/cwd/subdir") });
}

#[test]
fn saved_relative_is_absolutized_against_cwd() {
    let r = rp(None, None, Some(true), false, Some(("rel/left", "/abs/right")), "/cwd");
    assert_eq!(r, StartupPaths { left: p("/cwd/rel/left"), right: p("/abs/right") });
}

#[test]
fn dir1_with_dotdot_is_normalized() {
    let r = rp(Some("../../dir"), None, None, false, None, "/cwd/a/b");
    assert_eq!(r, StartupPaths { left: p("/cwd/dir"), right: p("/cwd/dir") });
}

#[test]
fn dir1_with_trailing_dot_is_normalized() {
    let r = rp(Some("."), None, None, false, None, "/cwd/a");
    assert_eq!(r, StartupPaths { left: p("/cwd/a"), right: p("/cwd/a") });
}

#[test]
fn normalize_lexical_collapses_dotdot() {
    let r = normalize_lexical(Path::new("/tmp/a/b/../../dir"));
    assert_eq!(r, p("/tmp/dir"));
}

#[test]
fn normalize_lexical_collapses_trailing_dot() {
    let r = normalize_lexical(Path::new("/tmp/a/b/."));
    assert_eq!(r, p("/tmp/a/b"));
}

#[test]
fn normalize_lexical_dotdot_above_root_stays_at_root() {
    let r = normalize_lexical(Path::new("/../../foo"));
    assert_eq!(r, p("/foo"));
}

#[test]
fn normalize_lexical_no_change_for_clean_path() {
    let r = normalize_lexical(Path::new("/tmp/a/b"));
    assert_eq!(r, p("/tmp/a/b"));
}

#[test]
fn absolutize_str_normalizes_absolute_input_with_dotdot() {
    let r = absolutize_str("/tmp/a/b/../../dir");
    assert_eq!(r, "/tmp/dir");
}

#[test]
fn abbreviate_home_exact_match() {
    let r = abbreviate_home("/home/alice", Some(Path::new("/home/alice")));
    assert_eq!(r, "~");
}

#[test]
fn abbreviate_home_subdir() {
    let r = abbreviate_home("/home/alice/projects", Some(Path::new("/home/alice")));
    assert_eq!(r, "~/projects");
}

#[test]
fn abbreviate_home_respects_segment_boundary() {
    let r = abbreviate_home("/home/alice2/projects", Some(Path::new("/home/alice")));
    assert_eq!(r, "/home/alice2/projects");
}

#[test]
fn abbreviate_home_unrelated_path_unchanged() {
    let r = abbreviate_home("/var/log", Some(Path::new("/home/alice")));
    assert_eq!(r, "/var/log");
}

#[test]
fn abbreviate_home_no_home_dir_unchanged() {
    let r = abbreviate_home("/var/log", None);
    assert_eq!(r, "/var/log");
}

// ── Search-history caching ────────────────────────────────────────────

fn search_query(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.to_string(),
        is_regex: false,
        case_sensitive: true,
        content: None,
        content_is_regex: false,
        content_case_sensitive: true,
        content_whole_words: false,
        max_depth: None,
        include_hidden: false,
        follow_symlinks: false,
    }
}

fn make_search_base(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("sc_app_search_test_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("target")).unwrap();
    std::fs::create_dir_all(base.join("dest")).unwrap();
    base
}

fn test_app(base: &Path) -> App {
    App::new(
        Config::default(),
        base.to_path_buf(),
        base.join("dest"),
        &AppState::default(),
        crate::panel_history::PanelHistory::default(),
        crate::panel_history::PanelHistory::default(),
        false,
    )
}

fn wait_for_search_done(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_search();
        if let PanelContent::SearchResults(sr) = &app.left.content {
            if !sr.running {
                return;
            }
        } else {
            return;
        }
        assert!(Instant::now() < deadline, "search did not finish in time");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn active_panel_shell_dir_uses_matched_files_directory_not_the_hidden_panel_path() {
    let base = make_search_base("shell_dir_matches");
    std::fs::write(base.join("target").join("hit.txt"), "needle here\n").unwrap();
    let mut app = test_app(&base);
    let query = SearchQuery { content: Some("needle".to_string()), ..search_query("*") };
    app.start_search(query);
    wait_for_search_done(&mut app);
    app.sync_matches_panel();
    app.active = Side::Right;
    assert!(matches!(app.right.content, PanelContent::Matches(_)));
    // The (hidden) panel.path is still `base/dest`, the pre-search directory —
    // the shell dir must be the matched file's directory instead.
    assert_eq!(app.right.path.0, base.join("dest").to_string_lossy());
    let expected = base.join("target").to_string_lossy().into_owned();
    assert_eq!(app.active_panel_shell_dir(), expected);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn execute_search_dialog_rejects_invalid_content_regex() {
    let base = make_search_base("invalid_content_regex");
    let mut app = test_app(&base);
    let mut state = SearchDialogState::new(false);
    state.content.text = "(".to_string();
    state.content.cursor = 1;
    state.content_is_regex = true;
    app.execute_search_dialog(state);
    match &app.modal {
        Modal::SearchDialog(s) => {
            assert!(s.error.as_deref().is_some_and(|e| e.starts_with("Invalid regex:")));
        }
        _ => panic!("expected the dialog to stay open with an error"),
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn search_dialog_reopen_reseeds_content_options() {
    let base = make_search_base("reopen_reseed");
    let mut app = test_app(&base);
    let mut query = search_query("*");
    query.content = Some("needle".to_string());
    query.content_is_regex = true;
    query.content_case_sensitive = false;
    query.content_whole_words = true;
    app.start_search(query);
    wait_for_search_done(&mut app);

    app.handle_action(Action::Search);
    match &app.modal {
        Modal::SearchDialog(s) => {
            assert!(s.content_is_regex);
            assert!(!s.content_case_sensitive);
            assert!(s.content_whole_words);
        }
        _ => panic!("expected the search dialog to reopen"),
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn jump_into_dir_hit_then_go_back_restores_cached_search() {
    let base = make_search_base("jump_back");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    assert_eq!(app.left.entries.len(), 1);
    assert_eq!(app.left.entries[0].name, "target");

    app.activate_search_hit(); // jump into the "target" dir hit
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.join("target").to_string_lossy());

    app.handle_action(Action::GoBack);
    match &app.left.content {
        PanelContent::SearchResults(sr) => {
            assert!(!sr.running);
            assert!(sr.complete);
        }
        _ => panic!("expected SearchResults after Alt-Left"),
    }
    assert_eq!(app.left.entries.len(), 1);
    assert_eq!(app.left.entries[0].name, "target");
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn back_back_forward_forward_retraces_symmetrically() {
    let base = make_search_base("bbff");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // now at base/target, Dir

    app.handle_action(Action::GoBack); // -> cached search at base
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    app.handle_action(Action::GoBack); // -> plain base (the startup history entry)
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    app.handle_action(Action::GoForward); // -> cached search at base again
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    app.handle_action(Action::GoForward); // -> back to base/target
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.join("target").to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn go_to_parent_on_live_search_only_closes_no_parent_nav() {
    let base = make_search_base("goparent_live");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    app.handle_action(Action::GoToParent);
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.to_string_lossy()); // not base's parent

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn go_to_parent_on_restored_cached_search_only_closes_no_parent_nav() {
    let base = make_search_base("goparent_cached");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // -> base/target
    app.handle_action(Action::GoBack); // -> cached search restored, at base
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    app.handle_action(Action::GoToParent);
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.to_string_lossy()); // not base's parent

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn esc_on_running_search_interrupts_without_closing() {
    let base = make_search_base("esc_interrupt");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    // No wait_for_search_done: sr.running only flips inside poll_search,
    // which we don't call, so it's still true here deterministically.
    match &app.left.content {
        PanelContent::SearchResults(sr) => assert!(sr.running),
        _ => panic!("expected a live SearchResults panel"),
    }

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    match &app.left.content {
        PanelContent::SearchResults(sr) => {
            assert!(!sr.running, "first Esc must interrupt, not just leave it running");
            assert!(!sr.complete, "an interrupted search isn't complete");
        }
        _ => panic!("first Esc must not close the search view"),
    }
    assert!(app.search.is_none(), "the worker must be cancelled");
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn second_esc_after_interrupt_closes_search() {
    let base = make_search_base("esc_interrupt_then_close");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)); // interrupt
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)); // close
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn esc_on_already_complete_search_closes_immediately() {
    let base = make_search_base("esc_complete_closes");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    match &app.left.content {
        PanelContent::SearchResults(sr) => {
            assert!(!sr.running);
            assert!(sr.complete);
        }
        _ => panic!("expected a completed SearchResults panel"),
    }

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(app.left.content, PanelContent::Dir), "a single Esc closes once already complete");
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn esc_on_matches_panel_interrupts_the_results_panel() {
    let base = make_search_base("esc_interrupt_via_matches");
    std::fs::write(base.join("target").join("hit.txt"), "needle here\n").unwrap();
    let mut app = test_app(&base);
    let query = SearchQuery { content: Some("needle".to_string()), ..search_query("*") };
    app.start_search(query);
    wait_for_search_done(&mut app);
    app.sync_matches_panel();
    // Re-open the search live (as if it were still running) after it
    // completed, so we can drive the interrupt path deterministically
    // while focus is on the Matches panel.
    if let PanelContent::SearchResults(sr) = &mut app.left.content {
        sr.running = true;
        sr.complete = false;
    }
    app.active = Side::Right;
    assert!(matches!(app.right.content, PanelContent::Matches(_)));

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    match &app.left.content {
        PanelContent::SearchResults(sr) => assert!(!sr.running, "results panel must be interrupted from the Matches panel too"),
        _ => panic!("first Esc must not close the search view"),
    }
    assert!(matches!(app.right.content, PanelContent::Matches(_)), "Matches panel must be untouched by an interrupt");

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert!(matches!(app.right.content, PanelContent::Dir));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn starting_new_search_drops_stale_forward_history() {
    let base = make_search_base("new_search_drops_forward");
    let mut app = test_app(&base);
    // Navigate into base/target, then back to base — target becomes a
    // forward-reachable entry.
    app.navigate_to_path(&base.join("target").to_string_lossy());
    assert_eq!(app.left.path.0, base.join("target").to_string_lossy());
    app.handle_action(Action::GoBack);
    assert_eq!(app.left.path.0, base.to_string_lossy());

    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    // The new search should have invalidated that stale forward entry:
    // Alt-Right must be a complete no-op, not a jump to base/target.
    app.handle_action(Action::GoForward);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn alt_right_on_fresh_search_with_no_forward_history_is_a_noop() {
    let base = make_search_base("fresh_search_no_forward");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    app.handle_action(Action::GoForward);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn alt_left_on_fresh_search_with_no_back_history_is_a_noop() {
    let base = make_search_base("fresh_search_no_back");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    app.handle_action(Action::GoBack);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn sync_panels_records_inactive_panel_history() {
    let base = make_search_base("sync_panels_history");
    let mut app = test_app(&base);
    // Right panel starts at base/dest. Navigate it into base/target, then
    // back to base/dest — base/target becomes a forward-reachable entry.
    app.active = Side::Right;
    app.navigate_to_path(&base.join("target").to_string_lossy());
    app.handle_action(Action::GoBack);
    assert_eq!(app.right.path.0, base.join("dest").to_string_lossy());

    // Sync the right (inactive) panel to the left panel's directory (base).
    app.active = Side::Left;
    app.handle_action(Action::SyncPanels);
    assert_eq!(app.right.path.0, base.to_string_lossy());

    // The sync must have been recorded in the right panel's own history:
    // forward no longer reaches the stale base/target entry, and back
    // returns to the pre-sync directory.
    app.active = Side::Right;
    app.handle_action(Action::GoForward);
    assert_eq!(app.right.path.0, base.to_string_lossy());

    app.handle_action(Action::GoBack);
    assert_eq!(app.right.path.0, base.join("dest").to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn double_click_into_directory_updates_panel_history() {
    let base = make_search_base("double_click_history");
    let mut app = test_app(&base);
    app.left_area.set(Rect::new(0, 0, 40, 20));
    // ".." sits first, then entries sorted by name ascending: "dest", "target".
    assert_eq!(app.left.entries[0].name, "..");
    assert_eq!(app.left.entries[1].name, "dest");
    assert_eq!(app.left.entries[2].name, "target");
    let col = 3;
    let row = 4; // inner_y=1 (border), header at inner_y, entries start at inner_y+1=2; row 4 = third entry ("target")

    app.handle_panel_down(col, row, MouseButton::Left);
    app.handle_panel_down(col, row, MouseButton::Left); // double-click within the 400ms window
    assert_eq!(app.left.path.0, base.join("target").to_string_lossy());

    // The double-click destination must have been recorded: Alt-Left
    // returns to the pre-click directory.
    app.handle_action(Action::GoBack);
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn new_search_evicts_previous_cached_search() {
    let base = make_search_base("evict");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // -> base/target (Dir); caches "target" search at base's slot

    app.start_search(search_query("*")); // clears the cache attached to base's slot
    app.close_search(); // back to Dir(base/target) without jumping (no new cache)

    app.handle_action(Action::GoBack);
    assert!(matches!(app.left.content, PanelContent::Dir));
    assert_eq!(app.left.path.0, base.to_string_lossy());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cache_snapshot_preserves_incomplete_flag_when_jumping_mid_scan() {
    let base = make_search_base("complete_flag");
    let mut app = test_app(&base);

    // Simulate a search interrupted mid-scan: still running, one hit in already.
    let mut sr = SearchResultsState::new(search_query("target"));
    sr.running = true;
    sr.complete = false;
    app.left.content = PanelContent::SearchResults(sr);
    app.left.entries = vec![NodeEntry {
        name: "target".into(),
        kind: NodeKind::Dir,
        size: 0,
        modified: std::time::SystemTime::UNIX_EPOCH,
        permissions: String::new(),
    }];
    app.left.cursor = 0;

    app.activate_search_hit(); // jump while the search was still "running"

    app.handle_action(Action::GoBack);
    match &app.left.content {
        PanelContent::SearchResults(sr) => assert!(!sr.complete, "should be cached as partial"),
        _ => panic!("expected SearchResults"),
    }

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn matches_panel_active_ignores_history_navigation() {
    let base = make_search_base("matches_noop");
    let mut app = test_app(&base);
    app.right.content = PanelContent::Matches(MatchesState::new("x".into(), false, false, false));
    app.active = Side::Right;
    let path_before = app.right.path.0.clone();

    app.handle_action(Action::GoBack);
    app.handle_action(Action::GoForward);
    app.handle_action(Action::GoToParent);

    assert!(matches!(app.right.content, PanelContent::Matches(_)));
    assert_eq!(app.right.path.0, path_before);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn toggle_matches_panel_hides_and_reshows_without_touching_the_search() {
    let base = make_search_base("toggle_matches");
    std::fs::write(base.join("target").join("hit.txt"), "needle here\n").unwrap();
    let mut app = test_app(&base);
    let query = SearchQuery { content: Some("needle".to_string()), ..search_query("*") };
    app.start_search(query);
    wait_for_search_done(&mut app);
    assert!(matches!(app.right.content, PanelContent::Matches(_)));

    app.handle_action(Action::ToggleMatchesPanel);
    assert!(matches!(app.right.content, PanelContent::Dir));
    assert!(app.matches_panel_hidden);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)), "hiding must not close the search");
    assert!(!app.status_active());

    app.handle_action(Action::ToggleMatchesPanel);
    assert!(matches!(app.right.content, PanelContent::Matches(_)));
    assert!(!app.matches_panel_hidden);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn delete_action_is_allowed_while_matches_panel_is_shown_and_syncs_it() {
    let base = make_search_base("delete_with_matches");
    std::fs::write(base.join("target").join("hit1.txt"), "needle one\n").unwrap();
    std::fs::write(base.join("target").join("hit2.txt"), "needle two\n").unwrap();
    let mut app = test_app(&base);
    let query = SearchQuery { content: Some("needle".to_string()), ..search_query("*") };
    app.start_search(query);
    wait_for_search_done(&mut app);
    assert_eq!(app.left.entries.len(), 2);
    assert!(matches!(app.right.content, PanelContent::Matches(_)));

    // Select and delete the first hit; F8 must not be refused for lacking a
    // "normal panel" destination — Delete has no destination.
    app.left.cursor = 0;
    let first_name = app.left.entries[0].name.clone();
    app.handle_action(Action::Delete);
    assert!(!app.status_active(), "Delete must not warn about a missing destination panel");
    let Modal::Confirm(state) = std::mem::replace(&mut app.modal, Modal::None) else {
        panic!("Delete should have opened a confirm dialog");
    };
    app.execute_file_op(state);

    assert_eq!(app.left.entries.len(), 1);
    assert!(!app.left.entries.iter().any(|e| e.name == first_name));

    // The matches panel must now reflect whatever is selected after the
    // delete, not the deleted file.
    let PanelContent::Matches(ms) = &app.right.content else { panic!("expected matches panel") };
    assert_ne!(ms.file.as_deref(), Some(first_name.as_str()));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn toggle_matches_panel_warns_when_not_applicable() {
    let base = make_search_base("toggle_matches_warn");
    let mut app = test_app(&base);

    app.handle_action(Action::ToggleMatchesPanel);
    assert!(app.status_active(), "no search at all should warn");

    app.start_search(search_query("*")); // name-only search, no `content`
    wait_for_search_done(&mut app);
    app.status = crate::ui::status_bar::StatusBarState::default();
    app.handle_action(Action::ToggleMatchesPanel);
    assert!(app.status_active(), "name-only search should warn too");
    assert!(matches!(app.right.content, PanelContent::Dir));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn jumping_again_from_restored_cache_does_not_leave_two_cached_searches() {
    let base = make_search_base("double_cache");
    std::fs::create_dir_all(base.join("target2")).unwrap();
    let mut app = test_app(&base);
    app.start_search(search_query("target*"));
    wait_for_search_done(&mut app);
    assert_eq!(app.left.entries.len(), 2);

    app.left.cursor = 0;
    app.activate_search_hit(); // jump into the first hit, caches search at base
    app.handle_action(Action::GoBack); // restore it
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    // Select the other hit and jump again from the restored view.
    let other = if app.left.entries[0].name == "target" { 1 } else { 0 };
    app.left.cursor = other;
    app.activate_search_hit();

    let resident = app.panel_history_left.caches.iter().filter(|c| c.is_some()).count();
    assert_eq!(resident, 1, "at most one cached search should ever be resident per side");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn deleting_a_hit_from_a_restored_cache_updates_the_cache_too() {
    let base = make_search_base("delete_syncs_cache");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // -> base/target, caches search at base
    app.handle_action(Action::GoBack); // restore cached search at base
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert_eq!(app.left.entries.len(), 1);

    app.left.tagged.insert("target".to_string());
    app.execute_file_op(ConfirmState::new(ConfirmOp::Delete, vec!["target".to_string()], None));
    assert!(app.left.entries.is_empty());

    // The CachedSearch attached to this history slot must be updated
    // too, not just the live view — otherwise a later replay resurrects
    // the deleted hit.
    let cache = app.panel_history_left.current_cache().expect("cache should still be attached");
    assert!(cache.entries.is_empty(), "deleted hit must be removed from the cache too");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn going_back_to_a_cache_whose_root_was_deleted_shows_an_error_instead_of_restoring() {
    let base = make_search_base("root_deleted");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // -> base/target, caches search rooted at base

    std::fs::remove_dir_all(&base).unwrap(); // the cached root no longer exists

    app.handle_action(Action::GoBack);
    assert!(
        !matches!(app.left.content, PanelContent::SearchResults(_)),
        "must not silently restore a search rooted at a directory that no longer exists"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn path_history_popup_leaves_search_open_until_a_pick_is_made() {
    let base = make_search_base("pathhist_guard");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // -> base/target
    app.handle_action(Action::GoBack); // restore cached search at base
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    // Opening the popup doesn't commit to navigating anywhere yet.
    app.handle_action(Action::PathHistory);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)), "search view must stay open while just browsing history");
    assert!(matches!(app.modal, Modal::PathHistoryList(_)));

    // Dismissing without picking anything must leave the search untouched.
    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(app.modal, Modal::None));
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)), "Esc must not have closed the search");

    // Picking an entry is the actual "go somewhere new" commitment.
    app.handle_action(Action::PathHistory);
    let expected = app.panel_history_left.unique_entries()[0].to_string();
    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(app.left.content, PanelContent::Dir), "search view must be closed once a pick is made");
    assert_eq!(app.left.path.0, expected);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn ipc_filter_is_refused_while_a_search_panel_is_active() {
    let base = make_search_base("ipc_filter_guard");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));

    app.handle_ipc(crate::ipc::IpcMessage::Filter("*.rs".to_string()));
    assert!(app.left.filter.is_none(), "filter must stay unset while a search panel is active");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn restoring_a_cached_search_reapplies_the_panels_current_sort() {
    let base = make_search_base("resort_on_restore");
    std::fs::create_dir_all(base.join("a_target")).unwrap();
    let mut app = test_app(&base);
    app.start_search(search_query("*target*"));
    wait_for_search_done(&mut app);
    assert_eq!(app.left.entries.len(), 2);
    app.left.cursor = 0;
    app.activate_search_hit(); // -> Dir; caches the search at base in ascending order

    app.left.sort_asc = false; // changed after the cache was captured
    app.handle_action(Action::GoBack); // restore; must resort per current settings

    let names: Vec<&str> = app.left.entries.iter().map(|e| e.name.as_str()).collect();
    let mut expected = names.clone();
    expected.sort_by_key(|b| std::cmp::Reverse(b.to_lowercase()));
    assert_eq!(names, expected, "restored entries should honor the panel's current sort");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn restoring_a_cached_search_clears_a_stale_panel_error() {
    let base = make_search_base("clear_stale_error");
    let mut app = test_app(&base);
    app.start_search(search_query("target"));
    wait_for_search_done(&mut app);
    app.activate_search_hit(); // -> base/target, caches search at base
    app.left.error = Some("stale error from an earlier failed listing".to_string());

    app.handle_action(Action::GoBack);
    assert!(matches!(app.left.content, PanelContent::SearchResults(_)));
    assert!(app.left.error.is_none(), "restoring a cached search must clear a stale panel error");

    let _ = std::fs::remove_dir_all(&base);
}

fn panels_cfg(exec: &str, text: &str, fallback: &str) -> crate::config::PanelsConfig {
    crate::config::PanelsConfig {
        default_action_executable: exec.to_string(),
        default_action_text: text.to_string(),
        default_action: fallback.to_string(),
        ..crate::config::PanelsConfig::default()
    }
}

#[test]
fn resolve_default_action_template_uses_specific_field_when_set() {
    let cfg = panels_cfg("run %f", ":view", "true");
    assert_eq!(resolve_default_action_template(true, &cfg), "run %f");
    assert_eq!(resolve_default_action_template(false, &cfg), ":view");
}

#[test]
fn resolve_default_action_template_falls_back_when_specific_field_empty() {
    let cfg = panels_cfg("", "", "echo fallback");
    assert_eq!(resolve_default_action_template(true, &cfg), "echo fallback");
    assert_eq!(resolve_default_action_template(false, &cfg), "echo fallback");
}

#[test]
fn resolve_default_action_template_empty_fallback_is_noop() {
    let cfg = panels_cfg("", "", "");
    assert_eq!(resolve_default_action_template(true, &cfg), "");
    assert_eq!(resolve_default_action_template(false, &cfg), "");
}

#[test]
fn view_action_opens_current_entry_in_viewer() {
    let base = make_search_base("view_action");
    std::fs::write(base.join("hello.txt"), "hello world\n").unwrap();
    let mut app = test_app(&base);
    let idx = app.left.entries.iter().position(|e| e.name == "hello.txt").unwrap();
    app.left.cursor = idx;
    app.handle_action(Action::View);
    let viewer = app.viewer.as_ref().expect("Action::View should open the viewer");
    assert!(viewer.text.contains("hello world"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn view_action_resets_scroll_for_a_newly_opened_file() {
    let base = make_search_base("view_action_scroll_reset");
    std::fs::write(base.join("a.txt"), "a\n").unwrap();
    std::fs::write(base.join("b.txt"), "b\n").unwrap();
    let mut app = test_app(&base);

    let idx_a = app.left.entries.iter().position(|e| e.name == "a.txt").unwrap();
    app.left.cursor = idx_a;
    app.handle_action(Action::View);
    app.overlay.scroll = 42; // simulate having scrolled down while viewing a.txt

    let idx_b = app.left.entries.iter().position(|e| e.name == "b.txt").unwrap();
    app.left.cursor = idx_b;
    app.handle_action(Action::View);
    assert_eq!(app.overlay.scroll, 0, "opening a new file must not inherit the previous file's scroll position");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn view_action_refuses_a_directory_entry() {
    let base = make_search_base("view_action_dir");
    let mut app = test_app(&base);
    let idx = app.left.entries.iter().position(|e| e.name == "target").unwrap();
    app.left.cursor = idx;
    app.handle_action(Action::View);
    assert!(app.viewer.is_none());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn enter_on_non_executable_file_opens_viewer_via_default_text_action() {
    let base = make_search_base("enter_default_view");
    std::fs::write(base.join("notes.txt"), "some notes\n").unwrap();
    let mut app = test_app(&base);
    let idx = app.left.entries.iter().position(|e| e.name == "notes.txt").unwrap();
    app.left.cursor = idx;
    app.open_current_entry_default_action();
    let viewer = app.viewer.as_ref().expect("default_action_text=:view should open the viewer");
    assert!(viewer.text.contains("some notes"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn enter_while_quicksearch_active_also_opens_viewer_via_default_text_action() {
    // Regression: quicksearch's own Enter handling calls PanelState::handle_key
    // directly, which historically bypassed the App-level default-action dispatch
    // that a plain Enter (without quicksearch active) goes through.
    let base = make_search_base("quicksearch_enter_default_view");
    std::fs::write(base.join("notes.txt"), "some notes\n").unwrap();
    let mut app = test_app(&base);
    let idx = app.left.entries.iter().position(|e| e.name == "notes.txt").unwrap();
    app.left.cursor = idx;
    app.quicksearch = Some("notes.txt".to_string());
    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.quicksearch.is_none(), "Enter should always close the quicksearch prompt");
    let viewer = app.viewer.as_ref().expect("Enter on a file during quicksearch should open the viewer");
    assert!(viewer.text.contains("some notes"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn enter_on_executable_text_file_view_it_by_default() {
    use std::os::unix::fs::PermissionsExt;
    let executable_text_content = "#!/bin/sh\necho hi\n";
    let base = make_search_base("enter_default_text_exec");
    let script = base.join("run.sh");
    std::fs::write(&script, executable_text_content).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = test_app(&base);
    let idx = app.left.entries.iter().position(|e| e.name == "run.sh").unwrap();
    app.left.cursor = idx;
    app.open_current_entry_default_action();
    assert!(app.viewer.is_some_and(|v| v.text == executable_text_content), "default_action_executable is empty and falls back to default_action_text = \":view\"");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn enter_on_executable_binary_is_a_noop_by_default() {
    use std::os::unix::fs::PermissionsExt;
    let base = make_search_base("enter_default_exec");
    let script = base.join("a_binary_exe.bin");
    std::fs::write(&script, "\0dummy\0content\0with\0nulls").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = test_app(&base);
    let idx = app.left.entries.iter().position(|e| e.name == "a_binary_exe.bin").unwrap();
    app.left.cursor = idx;
    app.open_current_entry_default_action();
    assert!(app.viewer.is_none(), "default_action_executable is empty and falls back to an empty default_action");

    let _ = std::fs::remove_dir_all(&base);
}

// ── Command palette ───────────────────────────────────────────────────────

fn tap(app: &mut App, code: KeyCode) {
    app.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

fn type_str(app: &mut App, text: &str) {
    for c in text.chars() {
        tap(app, KeyCode::Char(c));
    }
}

fn palette_items(app: &App) -> Vec<String> {
    app.palette.as_ref().unwrap().list.items.clone()
}

#[test]
fn every_keybinding_action_is_documented_in_the_cheatsheet_and_vice_versa() {
    let base = make_search_base("palette_docs");
    let app = test_app(&base);
    let known: Vec<&str> = app.bindings_list().iter().map(|(n, _, _)| *n).collect();
    let documented: Vec<String> = crate::palette::parse_cheatsheet(include_str!("../../docs/CheatSheet.md"))
        .into_iter().map(|c| c.config_key).collect();
    for d in &documented {
        assert!(known.contains(&d.as_str()), "CheatSheet lists unknown config key {d}");
    }
    for k in &known {
        assert!(documented.iter().any(|d| d == k), "config key {k} is missing from CheatSheet.md");
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn f1_opens_root_with_categories_and_f2_only_user_commands() {
    let base = make_search_base("palette_open");
    let mut app = test_app(&base);
    app.config.menu = vec![crate::config::MenuItem {
        label: "Say hi".into(), command: "echo hi".into(), keys: Some("F11".into()), add_to_bar: false,
    }];
    tap(&mut app, KeyCode::F(1));
    let items = palette_items(&app);
    assert!(items.iter().any(|i| i == "\u{25b8} File Operations"), "{items:?}");
    assert!(items.iter().any(|i| i == "\u{25b8} User Commands"));
    tap(&mut app, KeyCode::Esc);
    assert!(app.palette.is_none());

    app.open_palette(PaletteView::UserOnly);
    assert_eq!(palette_items(&app), vec!["Say hi"]);
    assert_eq!(app.palette.as_ref().unwrap().list.hints, vec!["F11"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn typing_fuzzy_filters_and_shows_the_live_shortcut() {
    let base = make_search_base("palette_search");
    let mut app = test_app(&base);
    app.config.keybindings.sort_panel = vec![KeyBinding::Single(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT))];
    app.open_palette(PaletteView::Root);
    type_str(&mut app, "sortpan");
    let s = app.palette.as_ref().unwrap();
    assert!(s.list.items[0].starts_with("Panels: Sort Panel \u{2013} Open sort popup"), "{:?}", s.list.items[0]);
    assert_eq!(s.list.hints[0], "Alt-z");
    type_str(&mut app, "qqqq");
    assert_eq!(palette_items(&app), vec!["No matching actions"]);
    tap(&mut app, KeyCode::Enter);
    assert!(app.palette.is_some(), "Enter on the placeholder row does nothing");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn category_drill_down_and_back() {
    let base = make_search_base("palette_category");
    let mut app = test_app(&base);
    app.open_palette(PaletteView::Root);
    let idx = palette_items(&app).iter().position(|i| i == "\u{25b8} Bookmarks").unwrap();
    app.palette.as_mut().unwrap().list.selected = idx;
    tap(&mut app, KeyCode::Enter);
    assert_eq!(palette_items(&app), vec![
        "Open Bookmarks \u{2013} Open bookmarks popup to navigate the active panel to a bookmarked directory",
        "Add Bookmark \u{2013} Add the active panel's current directory to bookmarks",
    ]);
    tap(&mut app, KeyCode::Esc);
    assert!(app.palette.as_ref().is_some_and(|s| s.view == PaletteView::Root));
    assert_eq!(palette_items(&app)[app.palette.as_ref().unwrap().list.selected], "\u{25b8} Bookmarks");

    // Backspace on an empty query leaves the category the same way.
    tap(&mut app, KeyCode::Enter);
    tap(&mut app, KeyCode::Backspace);
    assert_eq!(palette_items(&app)[app.palette.as_ref().unwrap().list.selected], "\u{25b8} Bookmarks");

    // So does Esc after typing a query inside the category.
    tap(&mut app, KeyCode::Enter);
    type_str(&mut app, "add");
    tap(&mut app, KeyCode::Esc);
    let s = app.palette.as_ref().unwrap();
    assert!(s.query.is_empty() && s.view == PaletteView::Root);
    assert_eq!(s.list.items[s.list.selected], "\u{25b8} Bookmarks");
    tap(&mut app, KeyCode::Esc);
    assert!(app.palette.is_none());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn enter_runs_the_action_and_records_it_as_recent() {
    let base = make_search_base("palette_run");
    let mut app = test_app(&base);
    let before = app.show_button_bar;
    app.open_palette(PaletteView::Root);
    type_str(&mut app, "togglebuttonbar");
    assert_eq!(palette_items(&app)[0], "Layout: Toggle Button Bar \u{2013} Toggle button bar visibility");
    tap(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none());
    assert_ne!(app.show_button_bar, before);
    assert_eq!(app.recent_actions, vec!["toggle_button_bar"]);

    app.open_palette(PaletteView::Root);
    assert_eq!(palette_items(&app)[0], "\u{21ba} Toggle Button Bar \u{2013} Toggle button bar visibility");
    assert_eq!(app.palette.as_ref().unwrap().list.hints[0], "Alt-b");
    let _ = std::fs::remove_dir_all(&base);
}

// ── Selecting the directory we came from ──────────────────────────────────

#[test]
fn go_to_parent_and_history_back_select_the_directory_we_left() {
    let base = make_search_base("select_left_dir");
    std::fs::create_dir_all(base.join("aaa")).unwrap();
    std::fs::create_dir_all(base.join("zzz").join("inner")).unwrap();
    let mut app = test_app(&base);

    app.navigate_to_path(&base.join("zzz").to_string_lossy());
    app.handle_action(Action::GoToParent);
    assert_eq!(app.left.path.0, base.to_string_lossy());
    assert_eq!(app.left.current_name(), "zzz");

    // Alt-Left to the parent selects it too, including across several levels.
    app.navigate_to_path(&base.join("zzz").join("inner").to_string_lossy());
    app.navigate_to_path(&base.to_string_lossy()); // unrelated-history jump to an ancestor
    assert_eq!(app.left.current_name(), "zzz");
    app.handle_action(Action::GoBack); // -> zzz/inner
    app.handle_action(Action::GoBack); // -> the earlier `base`, an ancestor of inner
    assert_eq!(app.left.path.0, base.to_string_lossy());
    assert_eq!(app.left.current_name(), "zzz");

    // Not a parent/child move: cursor starts at the top.
    app.navigate_to_path(&base.join("dest").to_string_lossy());
    assert_eq!(app.left.cursor, 0);
    let _ = std::fs::remove_dir_all(&base);
}
