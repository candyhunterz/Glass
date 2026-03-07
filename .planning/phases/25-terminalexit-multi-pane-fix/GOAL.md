Fix TerminalExit handler to use close_pane() for multi-pane tabs instead of close_tab(), completing SPLIT-11 satisfaction. Mirror the existing Ctrl+Shift+W pane-count check logic.
