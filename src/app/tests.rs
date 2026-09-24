use super::*;

#[test]
fn workspace_view_keeps_sidebar_selection_exclusive() {
    let first = SessionLocation {
        project_index: 0,
        agent_index: 0,
    };
    let second = SessionLocation {
        project_index: 0,
        agent_index: 1,
    };
    let conversation = WorkspaceView::Conversation(first);
    assert_eq!(conversation.highlighted_session(), Some(first));
    let archive = conversation.toggle_archive();
    assert_eq!(archive.highlighted_session(), None);
    assert_eq!(archive.displayed_session(), Some(first));
    assert_eq!(archive.toggle_archive(), conversation);

    let folders = conversation.open_picker(PickerStep::Folders);
    assert_eq!(folders.highlighted_session(), None);
    assert_eq!(folders.displayed_session(), None);
    assert_eq!(folders.return_to(), Some(first));

    let agents = folders.open_picker(PickerStep::Agents { project_index: 0 });
    assert_eq!(agents.highlighted_session(), None);
    assert_eq!(agents.return_to(), Some(first));

    assert_eq!(agents.toggle_archive(), archive);

    let updated = agents.session_archived(first, Some(second));
    assert_eq!(updated.highlighted_session(), None);
    assert_eq!(updated.return_to(), Some(second));
    assert_eq!(
        updated.toggle_archive().toggle_archive(),
        WorkspaceView::Conversation(second)
    );
    assert_eq!(
        conversation.session_archived(first, None),
        WorkspaceView::Empty
    );
}

#[test]
fn archive_icons_are_bundled() {
    assert!(
        gpui::AssetSource::load(&Assets, "icons/inbox.svg")
            .unwrap()
            .is_some()
    );
    assert!(
        gpui::AssetSource::load(&Assets, "icons/undo-2.svg")
            .unwrap()
            .is_some()
    );
}

fn agent(protocol: ProtocolVersion) -> AgentView {
    let mut agent = AgentView::new(AgentConfig {
        id: 1,
        command: vec!["fixture".into()],
        archived: false,
        display_name: None,
        session_id: None,
        model: None,
        context: None,
        messages: Vec::new(),
        available_commands: Vec::new(),
        pending_prompts: Vec::new(),
        was_working: false,
        session_has_activity: false,
    });
    agent.protocol = Some(protocol);
    agent.session_id = Some("session-1".into());
    agent.status = Status::Working;
    agent.active_work = true;
    agent
}

#[test]
fn v2_prompt_acknowledgement_is_not_completion() {
    let mut agent = agent(ProtocolVersion::V2);
    agent.awaiting_response = true;
    agent.handle_prompt_response(&json!({"messageId":"user-1"}));
    assert_eq!(agent.status, Status::Working);
    assert!(agent.active_work);
    assert!(agent.awaiting_response);

    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"agent_message","messageId":"agent-1","content":[{"type":"text","text":"Hello"}]
        }}}),
    );
    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"agent_message_chunk","messageId":"agent-1","content":{"type":"text","text":" world"}
        }}}),
    );
    assert_eq!(agent.messages.len(), 1);
    assert_eq!(agent.messages[0].text, "Hello world");
    assert!(!agent.awaiting_response);

    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"state_update","state":"idle","stopReason":"end_turn"
        }}}),
    );
    assert_eq!(agent.status, Status::Done);
    assert!(!agent.active_work);
}

#[test]
fn v1_prompt_response_still_completes_turn() {
    let mut agent = agent(ProtocolVersion::V1);
    agent.handle_prompt_response(&json!({"stopReason":"end_turn"}));
    assert_eq!(agent.status, Status::Done);
    assert!(!agent.active_work);
}

#[test]
fn sidebar_drag_moves_in_both_directions() {
    let mut order = vec![1, 2, 3, 4];
    assert!(move_sidebar_id(&mut order, 1, 3));
    assert_eq!(order, vec![2, 3, 1, 4]);
    assert!(move_sidebar_id(&mut order, 4, 2));
    assert_eq!(order, vec![4, 2, 3, 1]);
    assert!(!move_sidebar_id(&mut order, 4, 4));
}

#[test]
fn adjacent_tool_calls_form_one_group() {
    let mut agent = agent(ProtocolVersion::V2);
    agent.log(Role::Tool, "first tool");
    agent.log(Role::Tool, "second tool");
    agent.log(Role::Agent, "answer");
    agent.log(Role::Tool, "later tool");
    assert_eq!(tool_run_end(&agent.messages, 0), 2);
    assert_eq!(tool_run_end(&agent.messages, 3), 4);
    agent.config.archived = true;
    assert!(agent.snapshot().archived);
}

#[test]
fn tool_activity_summarizes_search_read_and_approval() {
    assert_eq!(
        tool_description("rg -n 'guardian|review|tool call' src"),
        ("Search guardian|review|tool call in src".into(), true)
    );
    assert_eq!(
        tool_description("/bin/bash -lc \"sed -n '1,80p' src/main.rs\""),
        ("Read src/main.rs".into(), true)
    );
    assert_eq!(
        tool_description("cd /project && head -80 README.md && ls && git log --oneline | wc -l"),
        ("Read README.md · List files · 2 more".into(), true)
    );
    let mut agent = agent(ProtocolVersion::V1);
    agent.log(Role::Tool, "rg -n guardian src");
    agent.log(Role::Tool, "Guardian Review");
    agent.log(Role::Tool, "cat README.md");
    assert_eq!(tool_group_heading(&agent.messages), "Ran");
    assert_eq!(
        approval_summary(&agent.messages).as_deref(),
        Some("1 approval check")
    );
    assert_eq!(markdown_code_block("a ``` b"), "````\na ``` b\n````");
}

#[test]
fn v1_tool_updates_replace_the_same_call_and_keep_approval_status() {
    let mut review_agent = agent(ProtocolVersion::V1);
    for update in [
        json!({"sessionUpdate":"tool_call","toolCallId":"guardian-1",
            "title":"Guardian Review","status":"in_progress"}),
        json!({"sessionUpdate":"tool_call_update","toolCallId":"guardian-1",
            "status":"completed"}),
    ] {
        Workspace::handle_update(
            &mut review_agent,
            &json!({"params":{"sessionId":"session-1","update":update}}),
        );
    }
    assert_eq!(review_agent.messages.len(), 1);
    assert_eq!(review_agent.messages[0].text, "Guardian Review · completed");
    assert_eq!(
        approval_summary(&review_agent.messages).as_deref(),
        Some("1 approval check · passed")
    );
    let mut tool = agent(ProtocolVersion::V2);
    tool.upsert_tool_call(
        &json!({"toolCallId":"command-1","title":"cat README.md","status":"in_progress"}),
    );
    tool.messages[0].text.push_str("\noutput text");
    tool.upsert_tool_call(&json!({"toolCallId":"command-1","status":"completed"}));
    assert_eq!(
        tool.messages[0].text,
        "cat README.md · completed\noutput text"
    );
}

#[test]
fn model_and_context_follow_acp_session_updates() {
    let mut agent = agent(ProtocolVersion::V1);
    let options = json!([{"id":"model","category":"model","currentValue":"default",
        "options":[{"value":"default","name":"Default","description":"Opus (1M context)"}]}]);
    assert_eq!(
        selected_model(&options).as_deref(),
        Some("Opus (1M context)")
    );
    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"usage_update","used":42_000,"size":200_000
        }}}),
    );
    assert_eq!(agent.context, Some((42_000, 200_000)));
}

#[test]
fn session_history_survives_config_round_trip() {
    let mut agent = agent(ProtocolVersion::V2);
    agent.model = Some("Example Model".into());
    agent.context = Some((3_000, 64_000));
    agent.log(Role::User, "Please check the build");
    agent.log(Role::Tool, "cargo test · completed");
    agent.config.pending_prompts.push("Follow up next".into());
    let saved = agent.snapshot();
    let encoded = serde_json::to_vec(&saved).unwrap();
    let restored = AgentView::new(serde_json::from_slice(&encoded).unwrap());
    assert_eq!(restored.config.session_id.as_deref(), Some("session-1"));
    assert_eq!(restored.model.as_deref(), Some("Example Model"));
    assert_eq!(restored.context, Some((3_000, 64_000)));
    assert_eq!(restored.messages[0].text, "Please check the build");
    assert_eq!(restored.messages[1].text, "cargo test · completed");
    assert!(restored.messages[2].text.contains("active"));
    assert_eq!(restored.config.pending_prompts, vec!["Follow up next"]);
}

#[test]
fn slash_commands_follow_agent_snapshots_and_complete_with_input_hint() {
    let mut agent = agent(ProtocolVersion::V1);
    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[
                {"name":"review","description":"Review changes","input":{"hint":"files"}},
                {"name":"reset","description":"Reset chat"}
            ]
        }}}),
    );
    assert_eq!(
        matching_slash_commands(&agent.config.available_commands, "/re").len(),
        2
    );
    assert_eq!(
        matching_slash_commands(&agent.config.available_commands, "/review ").len(),
        0
    );
    assert_eq!(
        completed_slash_text(&agent.config.available_commands[0]),
        "/review "
    );
    assert_eq!(
        completed_slash_text(&agent.config.available_commands[1]),
        "/reset"
    );

    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[
                {"name":"plan","description":"Plan work"}
            ]
        }}}),
    );
    assert_eq!(
        matching_slash_commands(&agent.config.available_commands, "/re").len(),
        0
    );

    let mut v2_agent = self::agent(ProtocolVersion::V2);
    Workspace::handle_update(
        &mut v2_agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[
                {"name":"plan","description":"Plan work"}
            ]
        }}}),
    );
    assert_eq!(v2_agent.config.available_commands[0].name, "plan");
}

#[test]
fn queued_prompt_stays_saved_when_agent_cannot_send_it() {
    let mut agent = agent(ProtocolVersion::V2);
    agent.config.pending_prompts.push("Next request".into());
    assert!(!agent.start_next_queued_prompt());
    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"state_update","state":"idle","stopReason":"end_turn"
        }}}),
    );
    assert!(agent.start_next_queued_prompt());
    assert_eq!(agent.status, Status::Error);
    assert_eq!(agent.config.pending_prompts, vec!["Next request"]);
}

#[test]
fn queued_prompts_reach_the_agent_in_order_after_each_turn() {
    let mut agent = agent(ProtocolVersion::V1);
    let command = vec![
        "/bin/sh".into(),
        "-c".into(),
        "while IFS= read -r line; do printf '%s\\n' \"$line\"; done".into(),
    ];
    let (events_tx, events_rx) = mpsc::channel();
    agent.connection = Some(Connection::spawn(1, &command, Path::new("/"), events_tx).unwrap());
    agent.config.pending_prompts = vec!["First".into(), "Second".into()];

    assert!(!agent.start_next_queued_prompt());
    agent.handle_prompt_response(&json!({"stopReason":"end_turn"}));
    for expected in ["First", "Second"] {
        assert!(agent.start_next_queued_prompt());
        let Event::Message { value, .. } = events_rx.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("agent disconnected before receiving queued prompt");
        };
        assert_eq!(value["params"]["prompt"][0]["text"], expected);
        agent.handle_prompt_response(&json!({"stopReason":"end_turn"}));
    }
    assert!(agent.config.pending_prompts.is_empty());
}

#[test]
fn restoration_respects_v1_capabilities() {
    assert_eq!(
        restore_mode(ProtocolVersion::V2, &json!({})),
        Some(RestoreMode::Resume)
    );
    assert_eq!(
        restore_mode(
            ProtocolVersion::V1,
            &json!({"agentCapabilities":{"sessionCapabilities":{"resume":{}},"loadSession":true}})
        ),
        Some(RestoreMode::Resume)
    );
    assert_eq!(
        restore_mode(
            ProtocolVersion::V1,
            &json!({"agentCapabilities":{"loadSession":true}})
        ),
        Some(RestoreMode::Load)
    );
    assert_eq!(restore_mode(ProtocolVersion::V1, &json!({})), None);
}

#[test]
fn v1_load_replay_does_not_duplicate_saved_history() {
    let mut agent = agent(ProtocolVersion::V1);
    agent.restoring = Some(RestoreMode::Load);
    agent.log(Role::Agent, "Saved reply");
    Workspace::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Saved reply"}
        }}}),
    );
    assert_eq!(agent.messages.len(), 1);
    assert_eq!(agent.messages[0].text, "Saved reply");
}

#[test]
fn empty_sessions_are_not_restored() {
    let mut agent = agent(ProtocolVersion::V2);
    agent.active_work = false;
    agent.log(Role::System, "A prior restore attempt failed");
    assert!(!agent.has_restorable_activity());
    agent.log(Role::User, "Continue this conversation");
    assert!(agent.has_restorable_activity());
}

#[test]
fn submitting_keeps_explicit_newlines() {
    assert_eq!(submitted_prompt("first\nsecond\n"), "first\nsecond");
    assert_eq!(submitted_prompt("first\n\n"), "first\n");
}
