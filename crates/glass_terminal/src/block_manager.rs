//! Block manager for shell integration command lifecycle tracking.
//!
//! Tracks commands through PromptActive -> InputActive -> Executing -> Complete
//! states, recording line ranges, exit codes, and timing for duration display.

use std::time::{Duration, Instant};

use crate::osc_scanner::OscEvent;
use glass_pipes::CapturedStage;

/// State of a command block in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    /// Shell prompt is being displayed
    PromptActive,
    /// User is typing a command
    InputActive,
    /// Command is executing
    Executing,
    /// Command has finished
    Complete,
}

/// A single command block representing one prompt-command-output cycle.
#[derive(Debug, Clone)]
pub struct Block {
    /// Line where the prompt started
    pub prompt_start_line: usize,
    /// Line where command input started
    pub command_start_line: usize,
    /// Line where command output started (after execution began)
    pub output_start_line: Option<usize>,
    /// Line where command output ended
    pub output_end_line: Option<usize>,
    /// Exit code from the command (if finished)
    pub exit_code: Option<i32>,
    /// When command execution started
    pub started_at: Option<Instant>,
    /// When command execution finished
    pub finished_at: Option<Instant>,
    /// Wall-clock epoch seconds when command started (for matching with history DB records).
    pub started_epoch: Option<i64>,
    /// Current lifecycle state
    pub state: BlockState,
    /// Whether a pre-exec snapshot exists for this command (enables [undo] label).
    pub has_snapshot: bool,
    /// Captured pipeline stages (empty for non-pipeline commands).
    pub pipeline_stages: Vec<CapturedStage>,
    /// Expected number of pipeline stages (from OSC 133;S).
    pub pipeline_stage_count: Option<usize>,
    /// Whether the pipeline block is expanded (showing stage rows).
    pub pipeline_expanded: bool,
    /// Per-stage command text (parallel to pipeline_stages).
    pub pipeline_stage_commands: Vec<String>,
    /// Which single stage is showing full captured output (None = all collapsed).
    pub expanded_stage_index: Option<usize>,
    /// SOI one-line summary for this block (set after SOI parse completes).
    pub soi_summary: Option<String>,
    /// SOI severity string: "Error" | "Warning" | "Info" | "Success" (None until set).
    pub soi_severity: Option<String>,
}

impl Block {
    fn new(prompt_line: usize) -> Self {
        Self {
            prompt_start_line: prompt_line,
            command_start_line: prompt_line,
            output_start_line: None,
            output_end_line: None,
            exit_code: None,
            started_at: None,
            finished_at: None,
            started_epoch: None,
            state: BlockState::PromptActive,
            has_snapshot: false,
            pipeline_stages: Vec::new(),
            pipeline_stage_count: None,
            pipeline_expanded: false,
            pipeline_stage_commands: Vec::new(),
            expanded_stage_index: None,
            soi_summary: None,
            soi_severity: None,
        }
    }

    /// Calculate the duration of command execution.
    pub fn duration(&self) -> Option<Duration> {
        match (self.started_at, self.finished_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }

    /// Toggle the pipeline expand/collapse state.
    /// Clears expanded_stage_index when collapsing.
    pub fn toggle_pipeline_expanded(&mut self) {
        self.pipeline_expanded = !self.pipeline_expanded;
        if !self.pipeline_expanded {
            self.expanded_stage_index = None;
        }
    }

    /// Set which stage is showing full captured output, or None to collapse all.
    pub fn set_expanded_stage(&mut self, index: Option<usize>) {
        self.expanded_stage_index = index;
    }

    /// Number of overlay rows this pipeline uses.
    /// 0 if collapsed or non-pipeline, stage_count if expanded.
    pub fn pipeline_row_count(&self) -> usize {
        if !self.pipeline_expanded {
            return 0;
        }
        if !self.pipeline_stage_commands.is_empty() {
            self.pipeline_stage_commands.len()
        } else {
            self.pipeline_stages.len()
        }
    }
}

/// Result of a hit test on pipeline stage rows.
#[derive(Debug, PartialEq)]
pub enum PipelineHit {
    /// Clicked on the separator line of a pipeline block
    Header,
    /// Clicked on a specific stage row
    StageRow(usize),
}

/// Maximum number of blocks retained in memory per session.
/// Prevents unbounded memory growth in very long-running sessions.
const MAX_BLOCKS: usize = 10_000;

/// Manages the collection of command blocks.
pub struct BlockManager {
    blocks: Vec<Block>,
    current: Option<usize>,
    /// Last known terminal column count; used to detect reflow-inducing resizes.
    last_columns: usize,
}

impl BlockManager {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            current: None,
            last_columns: 0,
        }
    }

    /// Notify the block manager that the terminal was resized.
    /// We keep existing blocks even when column count changes — their line
    /// positions may be slightly off after reflow, but clearing them would
    /// permanently lose all decorations (separators, badges, etc.) since
    /// shell integration only re-emits OSC 133 for the current prompt.
    pub fn notify_resize(&mut self, columns: usize) {
        self.last_columns = columns;
    }

    /// Process an OSC event and update block state.
    pub fn handle_event(&mut self, event: &OscEvent, line: usize) {
        match event {
            OscEvent::PromptStart => {
                // Prune oldest blocks to prevent unbounded memory growth
                if self.blocks.len() >= MAX_BLOCKS {
                    let drain_count = MAX_BLOCKS / 10; // Remove 10% at a time
                    self.blocks.drain(..drain_count);
                    // Adjust current index after drain
                    self.current = self.current.and_then(|idx| idx.checked_sub(drain_count));
                }
                let block = Block::new(line);
                self.blocks.push(block);
                self.current = Some(self.blocks.len() - 1);
            }
            OscEvent::CommandStart => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
                        if block.state != BlockState::PromptActive {
                            tracing::warn!(
                                "Unexpected CommandStart in {:?} state (expected PromptActive)",
                                block.state
                            );
                        }
                        block.state = BlockState::InputActive;
                        block.command_start_line = line;
                    }
                }
            }
            OscEvent::CommandExecuted => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
                        if block.state != BlockState::InputActive {
                            tracing::warn!(
                                "Unexpected CommandExecuted in {:?} state (expected InputActive)",
                                block.state
                            );
                        }
                        block.state = BlockState::Executing;
                        block.output_start_line = Some(line);
                        block.started_at = Some(Instant::now());
                        block.started_epoch = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_secs() as i64);
                    }
                }
            }
            OscEvent::CommandFinished { exit_code } => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
                        if block.state != BlockState::Executing {
                            tracing::warn!(
                                "Unexpected CommandFinished in {:?} state (expected Executing)",
                                block.state
                            );
                        }
                        block.state = BlockState::Complete;
                        block.exit_code = *exit_code;
                        block.output_end_line = Some(line);
                        block.finished_at = Some(Instant::now());

                        // Auto-expand pipeline blocks on failure or >2 stages
                        if block.pipeline_stage_count.unwrap_or(0) > 0
                            || block.pipeline_stage_commands.len() > 1
                        {
                            let stage_count = block
                                .pipeline_stage_count
                                .unwrap_or(0)
                                .max(block.pipeline_stage_commands.len());
                            let failed = block.exit_code.is_some_and(|c| c != 0);
                            block.pipeline_expanded = failed || stage_count > 2;
                        }
                    }
                }
            }
            // CurrentDirectory events are handled by StatusState, not BlockManager
            OscEvent::CurrentDirectory(_) => {}
            OscEvent::PipelineStart { stage_count } => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
                        block.pipeline_stage_count = Some(*stage_count);
                        block.pipeline_stages = Vec::with_capacity(*stage_count);
                    }
                }
            }
            OscEvent::PipelineStage {
                index,
                total_bytes,
                temp_path,
            } => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
                        block.pipeline_stages.push(CapturedStage {
                            index: *index,
                            total_bytes: *total_bytes,
                            data: glass_pipes::FinalizedBuffer::Complete(Vec::new()),
                            temp_path: Some(temp_path.clone()),
                        });
                    }
                }
            }
        }
    }

    /// Return blocks that overlap the given viewport.
    pub fn visible_blocks(&self, display_offset: usize, screen_lines: usize) -> Vec<&Block> {
        let viewport_start = display_offset;
        let viewport_end = display_offset.saturating_add(screen_lines);

        self.blocks
            .iter()
            .filter(|block| {
                let block_start = block.prompt_start_line;
                let block_end = block
                    .output_end_line
                    .or(block.output_start_line)
                    .unwrap_or(block.command_start_line);
                // Block overlaps viewport if it starts before viewport ends
                // and ends at or after viewport starts
                block_start < viewport_end && block_end >= viewport_start
            })
            .collect()
    }

    /// Evict heavyweight data (pipeline stages, stage commands) from blocks
    /// that are far from the current viewport. This prevents unbounded memory
    /// growth when many pipeline-heavy commands have scrolled off-screen.
    ///
    /// Blocks within `margin` lines of the viewport are left intact.
    pub fn evict_distant_blocks(&mut self, display_offset: usize, screen_lines: usize) {
        const EVICTION_MARGIN: usize = 1000;
        let viewport_start = display_offset.saturating_sub(EVICTION_MARGIN);
        let viewport_end = display_offset
            .saturating_add(screen_lines)
            .saturating_add(EVICTION_MARGIN);

        for block in &mut self.blocks {
            let block_end = block
                .output_end_line
                .or(block.output_start_line)
                .unwrap_or(block.command_start_line);
            // Block is distant if it ends before the extended viewport starts
            // or starts after the extended viewport ends
            if block_end < viewport_start || block.prompt_start_line > viewport_end {
                if !block.pipeline_stages.is_empty() {
                    block.pipeline_stages = Vec::new();
                }
                if !block.pipeline_stage_commands.is_empty() {
                    block.pipeline_stage_commands = Vec::new();
                }
            }
        }
    }

    /// Get all blocks.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Get a mutable reference to all blocks.
    pub fn blocks_mut(&mut self) -> &mut Vec<Block> {
        &mut self.blocks
    }

    /// Get the current block index.
    pub fn current_block_index(&self) -> Option<usize> {
        self.current
    }

    /// Get a mutable reference to a block by index.
    pub fn block_mut(&mut self, index: usize) -> Option<&mut Block> {
        self.blocks.get_mut(index)
    }

    /// Get a mutable reference to the current (most recent) block.
    pub fn current_block_mut(&mut self) -> Option<&mut Block> {
        self.current.and_then(|idx| self.blocks.get_mut(idx))
    }

    /// Find a block by its started_epoch and return a mutable reference.
    pub fn find_block_by_epoch_mut(&mut self, epoch: i64) -> Option<&mut Block> {
        self.blocks
            .iter_mut()
            .rev()
            .find(|b| b.started_epoch == Some(epoch))
    }

    /// Hit test pipeline stage panel at the bottom of the viewport.
    /// Returns (block_index, PipelineHit) if a pipeline row was clicked.
    ///
    /// `viewport_height` and `status_bar_height` are in pixels.
    pub fn pipeline_hit_test(
        &self,
        _x: f32,
        y: f32,
        _cell_w: f32,
        cell_h: f32,
        viewport_height: f32,
        status_bar_height: f32,
    ) -> Option<(usize, PipelineHit)> {
        // Find the last expanded pipeline block (matching renderer)
        let (block_idx, block) = self
            .blocks
            .iter()
            .enumerate()
            .rev()
            .find(|(_, b)| b.pipeline_expanded)?;

        let stage_count = if !block.pipeline_stage_commands.is_empty() {
            block.pipeline_stage_commands.len()
        } else if !block.pipeline_stages.is_empty() {
            block.pipeline_stages.len()
        } else {
            return None;
        };

        // Calculate total panel rows (matching renderer logic)
        let mut total_rows = stage_count;
        if let Some(idx) = block.expanded_stage_index {
            total_rows += self.expanded_output_row_count(block, idx);
        }

        let panel_top = viewport_height - status_bar_height - total_rows as f32 * cell_h;
        let mut row = 0;

        for si in 0..stage_count {
            let row_y = panel_top + row as f32 * cell_h;
            if y >= row_y && y < row_y + cell_h {
                return Some((block_idx, PipelineHit::StageRow(si)));
            }
            row += 1;

            // Skip expanded output rows
            if block.expanded_stage_index == Some(si) {
                row += self.expanded_output_row_count(block, si);
            }
        }

        None
    }

    /// Number of output rows for an expanded stage (mirrors renderer logic).
    fn expanded_output_row_count(&self, block: &Block, stage_idx: usize) -> usize {
        if let Some(stage) = block.pipeline_stages.get(stage_idx) {
            let lines = match &stage.data {
                glass_pipes::FinalizedBuffer::Complete(bytes) => {
                    let n = bytes.iter().filter(|&&b| b == b'\n').count();
                    n.max(if bytes.is_empty() { 0 } else { 1 })
                }
                glass_pipes::FinalizedBuffer::Sampled { head, tail, .. } => {
                    head.iter().filter(|&&b| b == b'\n').count()
                        + tail.iter().filter(|&&b| b == b'\n').count()
                        + 1 // omission indicator row
                }
                glass_pipes::FinalizedBuffer::Binary { .. } => 1,
            };
            if lines == 0 {
                1
            } else {
                lines.min(30)
            }
        } else {
            1 // "no captured data" row
        }
    }
}

impl Default for BlockManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the shell hint line for SOI summary injection.
/// Returns None if gating conditions are not met (disabled, shell_summary off, below min_lines, empty summary).
/// The returned string uses ANSI SGR dim formatting only (no OSC sequences).
pub fn build_soi_hint_line(
    summary: &str,
    enabled: bool,
    shell_summary: bool,
    min_lines: u32,
    raw_line_count: i64,
) -> Option<String> {
    if !enabled || !shell_summary || summary.is_empty() {
        return None;
    }
    if min_lines > 0 && raw_line_count < min_lines as i64 {
        return None;
    }
    // Truncate long summaries to prevent terminal buffer bloat
    let display = if summary.len() > 200 {
        format!("{}...", &summary[..197])
    } else {
        summary.to_string()
    };
    Some(format!("\x1b[2m[glass-soi] {}\x1b[0m\r\n", display))
}

/// Format a duration into a human-readable string.
pub fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs_f64();

    if total_secs < 0.001 {
        "<1ms".to_string()
    } else if total_secs < 1.0 {
        format!("{}ms", d.as_millis())
    } else if total_secs < 60.0 {
        format!("{:.1}s", total_secs)
    } else {
        let mins = d.as_secs() / 60;
        let secs = d.as_secs() % 60;
        format!("{}m {}s", mins, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc_scanner::OscEvent;
    use std::time::Duration;

    #[test]
    fn prompt_start_creates_block() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        assert_eq!(bm.blocks().len(), 1);
        assert_eq!(bm.blocks()[0].state, BlockState::PromptActive);
        assert_eq!(bm.blocks()[0].prompt_start_line, 0);
    }

    #[test]
    fn command_start_transitions_to_input_active() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        assert_eq!(bm.blocks()[0].state, BlockState::InputActive);
        assert_eq!(bm.blocks()[0].command_start_line, 1);
    }

    #[test]
    fn command_executed_transitions_to_executing() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        assert_eq!(bm.blocks()[0].state, BlockState::Executing);
        assert!(bm.blocks()[0].started_at.is_some());
    }

    #[test]
    fn command_finished_transitions_to_complete() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        assert_eq!(bm.blocks()[0].state, BlockState::Complete);
        assert_eq!(bm.blocks()[0].exit_code, Some(0));
        assert!(bm.blocks()[0].finished_at.is_some());
    }

    #[test]
    fn multiple_commands_create_multiple_blocks() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        // Second command
        bm.handle_event(&OscEvent::PromptStart, 6);
        bm.handle_event(&OscEvent::CommandStart, 7);
        assert_eq!(bm.blocks().len(), 2);
        assert_eq!(bm.blocks()[0].state, BlockState::Complete);
        assert_eq!(bm.blocks()[1].state, BlockState::InputActive);
    }

    #[test]
    fn visible_blocks_returns_overlapping() {
        let mut bm = BlockManager::new();
        // Block at lines 0-5
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        // Block at lines 10-15
        bm.handle_event(&OscEvent::PromptStart, 10);
        bm.handle_event(&OscEvent::CommandStart, 11);
        bm.handle_event(&OscEvent::CommandExecuted, 11);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 15);

        // Viewport covers lines 0-9 (should only see first block)
        let visible = bm.visible_blocks(0, 10);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].prompt_start_line, 0);

        // Viewport covers lines 0-15 (should see both blocks)
        let visible = bm.visible_blocks(0, 16);
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn duration_for_complete_block() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        // Manually set timing for deterministic test
        bm.blocks[0].started_at = Some(Instant::now());
        bm.blocks[0].finished_at =
            Some(bm.blocks[0].started_at.unwrap() + Duration::from_millis(1500));
        let d = bm.blocks[0].duration().unwrap();
        assert_eq!(d, Duration::from_millis(1500));
    }

    #[test]
    fn handle_event_without_prompt_start_is_resilient() {
        let mut bm = BlockManager::new();
        // Should not panic even without a prior PromptStart
        bm.handle_event(&OscEvent::CommandStart, 0);
        bm.handle_event(&OscEvent::CommandExecuted, 0);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(1) }, 0);
        // No blocks created — all events ignored gracefully
        assert!(bm.blocks().is_empty());
    }

    // format_duration tests
    #[test]
    fn format_duration_milliseconds() {
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_secs_f64(1.23)), "1.2s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
    }

    #[test]
    fn format_duration_sub_millisecond() {
        assert_eq!(format_duration(Duration::from_secs_f64(0.0005)), "<1ms");
    }

    // -- Pipeline stage tests --

    #[test]
    fn block_new_has_empty_pipeline_stages() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        assert!(bm.blocks()[0].pipeline_stages.is_empty());
        assert_eq!(bm.blocks()[0].pipeline_stage_count, None);
    }

    #[test]
    fn pipeline_start_sets_stage_count() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 3 }, 1);
        assert_eq!(bm.blocks()[0].pipeline_stage_count, Some(3));
    }

    #[test]
    fn pipeline_stage_adds_entry() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 2 }, 1);
        bm.handle_event(
            &OscEvent::PipelineStage {
                index: 0,
                total_bytes: 1024,
                temp_path: "/tmp/glass/stage_0".to_string(),
            },
            2,
        );
        assert_eq!(bm.blocks()[0].pipeline_stages.len(), 1);
        assert_eq!(bm.blocks()[0].pipeline_stages[0].index, 0);
        assert_eq!(bm.blocks()[0].pipeline_stages[0].total_bytes, 1024);
        assert_eq!(
            bm.blocks()[0].pipeline_stages[0].temp_path,
            Some("/tmp/glass/stage_0".to_string())
        );
    }

    #[test]
    fn multiple_pipeline_stages_accumulate() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 3 }, 1);
        for i in 0..3 {
            bm.handle_event(
                &OscEvent::PipelineStage {
                    index: i,
                    total_bytes: 100 * (i + 1),
                    temp_path: format!("/tmp/glass/stage_{}", i),
                },
                2,
            );
        }
        assert_eq!(bm.blocks()[0].pipeline_stages.len(), 3);
        assert_eq!(bm.blocks()[0].pipeline_stages[2].index, 2);
        assert_eq!(bm.blocks()[0].pipeline_stages[2].total_bytes, 300);
    }

    #[test]
    fn pipeline_events_without_current_block_ignored() {
        let mut bm = BlockManager::new();
        // No PromptStart -- no current block
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 2 }, 0);
        bm.handle_event(
            &OscEvent::PipelineStage {
                index: 0,
                total_bytes: 512,
                temp_path: "/tmp/glass/stage_0".to_string(),
            },
            1,
        );
        assert!(bm.blocks().is_empty());
    }

    // -- Pipeline UI state tests (Phase 17) --

    #[test]
    fn pipeline_auto_expand_on_failure() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 2 }, 1);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(1) }, 5);
        assert!(bm.blocks()[0].pipeline_expanded);
    }

    #[test]
    fn pipeline_auto_expand_on_many_stages() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 3 }, 1);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        assert!(bm.blocks()[0].pipeline_expanded);
    }

    #[test]
    fn pipeline_auto_collapse_simple_success() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 2 }, 1);
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        assert!(!bm.blocks()[0].pipeline_expanded);
    }

    #[test]
    fn pipeline_non_pipeline_stays_collapsed() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        // No PipelineStart event -- pipeline_stage_count stays None
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        assert!(!bm.blocks()[0].pipeline_expanded);
    }

    #[test]
    fn pipeline_stage_commands_stored() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        // Simulate external population of pipeline_stage_commands
        bm.current_block_mut().unwrap().pipeline_stage_commands =
            vec!["grep foo".to_string(), "wc -l".to_string()];
        assert_eq!(bm.blocks()[0].pipeline_stage_commands.len(), 2);
        assert_eq!(bm.blocks()[0].pipeline_stage_commands[0], "grep foo");
    }

    #[test]
    fn expanded_stage_index_defaults_none() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        assert_eq!(bm.blocks()[0].expanded_stage_index, None);
    }

    #[test]
    fn toggle_pipeline_expanded_flips() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        assert!(!bm.blocks()[0].pipeline_expanded);
        bm.current_block_mut().unwrap().toggle_pipeline_expanded();
        assert!(bm.blocks()[0].pipeline_expanded);
        bm.current_block_mut().unwrap().toggle_pipeline_expanded();
        assert!(!bm.blocks()[0].pipeline_expanded);
    }

    #[test]
    fn toggle_pipeline_expanded_clears_expanded_stage() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        let block = bm.current_block_mut().unwrap();
        block.pipeline_expanded = true;
        block.expanded_stage_index = Some(1);
        block.toggle_pipeline_expanded(); // collapse
        assert!(!block.pipeline_expanded);
        assert_eq!(block.expanded_stage_index, None);
    }

    #[test]
    fn set_expanded_stage_sets_and_clears() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        let block = bm.current_block_mut().unwrap();
        block.set_expanded_stage(Some(2));
        assert_eq!(block.expanded_stage_index, Some(2));
        block.set_expanded_stage(None);
        assert_eq!(block.expanded_stage_index, None);
    }

    // -- Pipeline hit test tests (UI-04) --

    /// Helper: create a BlockManager with one expanded pipeline block for hit testing.
    fn make_hit_test_manager(
        commands: Vec<&str>,
        expanded: bool,
        expanded_stage: Option<usize>,
    ) -> BlockManager {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        {
            let block = bm.current_block_mut().unwrap();
            block.pipeline_stage_commands = commands.iter().map(|s| s.to_string()).collect();
            block.pipeline_expanded = expanded;
            block.expanded_stage_index = expanded_stage;
        }
        bm
    }

    #[test]
    fn pipeline_hit_test_returns_none_when_collapsed() {
        let bm = make_hit_test_manager(vec!["cat", "grep"], false, None);
        // Click somewhere in the viewport
        let result = bm.pipeline_hit_test(100.0, 500.0, 8.0, 16.0, 600.0, 20.0);
        assert_eq!(result, None, "Collapsed pipeline should not produce hits");
    }

    #[test]
    fn pipeline_hit_test_returns_stage_row_for_correct_y() {
        let bm = make_hit_test_manager(vec!["cat file", "grep foo", "wc -l"], true, None);
        let cell_h = 16.0;
        let viewport_h = 600.0;
        let status_h = 20.0;
        // Panel with 3 stages: panel_top = 600 - 20 - 3*16 = 532
        let panel_top = viewport_h - status_h - 3.0 * cell_h;

        // Click on stage row 0 (first row in panel)
        let result =
            bm.pipeline_hit_test(100.0, panel_top + 1.0, 8.0, cell_h, viewport_h, status_h);
        assert_eq!(result, Some((0, PipelineHit::StageRow(0))));

        // Click on stage row 1
        let result = bm.pipeline_hit_test(
            100.0,
            panel_top + cell_h + 1.0,
            8.0,
            cell_h,
            viewport_h,
            status_h,
        );
        assert_eq!(result, Some((0, PipelineHit::StageRow(1))));

        // Click on stage row 2
        let result = bm.pipeline_hit_test(
            100.0,
            panel_top + 2.0 * cell_h + 1.0,
            8.0,
            cell_h,
            viewport_h,
            status_h,
        );
        assert_eq!(result, Some((0, PipelineHit::StageRow(2))));
    }

    #[test]
    fn pipeline_hit_test_returns_none_outside_panel() {
        let bm = make_hit_test_manager(vec!["cat", "grep"], true, None);
        let cell_h = 16.0;
        let viewport_h = 600.0;
        let status_h = 20.0;
        // Panel with 2 stages: panel_top = 600 - 20 - 2*16 = 548
        let panel_top = viewport_h - status_h - 2.0 * cell_h;

        // Click above the panel
        let result =
            bm.pipeline_hit_test(100.0, panel_top - 10.0, 8.0, cell_h, viewport_h, status_h);
        assert_eq!(result, None, "Click above panel should return None");

        // Click below the panel (in status bar area)
        let result = bm.pipeline_hit_test(
            100.0,
            viewport_h - status_h + 5.0,
            8.0,
            cell_h,
            viewport_h,
            status_h,
        );
        assert_eq!(result, None, "Click in status bar should return None");
    }

    #[test]
    fn pipeline_hit_test_skips_expanded_output_rows() {
        // When a stage is expanded, output rows appear between stage rows.
        // Clicking on an output row should NOT match a stage row.
        let mut bm = make_hit_test_manager(vec!["cat file", "grep foo"], true, Some(0));
        // Add a captured stage with 2 lines of output for stage 0
        {
            let block = bm.current_block_mut().unwrap();
            block.pipeline_stages.push(CapturedStage {
                index: 0,
                total_bytes: 12,
                data: glass_pipes::FinalizedBuffer::Complete(b"line1\nline2\n".to_vec()),
                temp_path: None,
            });
            block.pipeline_stages.push(CapturedStage {
                index: 1,
                total_bytes: 5,
                data: glass_pipes::FinalizedBuffer::Complete(b"hello".to_vec()),
                temp_path: None,
            });
        }

        let cell_h = 16.0;
        let viewport_h = 600.0;
        let status_h = 20.0;
        // total_rows = 2 stages + 2 output rows for stage 0 = 4
        // panel_top = 600 - 20 - 4*16 = 516
        let panel_top = viewport_h - status_h - 4.0 * cell_h;

        // Row 0 = stage 0 header
        let result =
            bm.pipeline_hit_test(100.0, panel_top + 1.0, 8.0, cell_h, viewport_h, status_h);
        assert_eq!(result, Some((0, PipelineHit::StageRow(0))));

        // Row 3 = stage 1 header (after stage 0 header + 2 output rows)
        let result = bm.pipeline_hit_test(
            100.0,
            panel_top + 3.0 * cell_h + 1.0,
            8.0,
            cell_h,
            viewport_h,
            status_h,
        );
        assert_eq!(result, Some((0, PipelineHit::StageRow(1))));
    }

    #[test]
    fn new_prompt_resets_pipeline_state() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 2 }, 1);
        bm.handle_event(
            &OscEvent::PipelineStage {
                index: 0,
                total_bytes: 100,
                temp_path: "/tmp/glass/stage_0".to_string(),
            },
            2,
        );
        // New prompt creates fresh block
        bm.handle_event(&OscEvent::PromptStart, 10);
        assert!(bm.blocks()[1].pipeline_stages.is_empty());
        assert_eq!(bm.blocks()[1].pipeline_stage_count, None);
        // First block still has its pipeline data
        assert_eq!(bm.blocks()[0].pipeline_stages.len(), 1);
    }

    // -- SOI hint line builder tests (Phase 52 Plan 02) --

    #[test]
    fn test_soi_hint_line_format() {
        let result = build_soi_hint_line("3 errors found", true, true, 0, 15);
        assert_eq!(
            result,
            Some("\x1b[2m[glass-soi] 3 errors found\x1b[0m\r\n".to_string())
        );
        // Verify no OSC sequences (\x1b]) present
        assert!(!result.as_ref().unwrap().contains("\x1b]"));
    }

    #[test]
    fn test_soi_hint_line_gating_disabled() {
        // shell_summary=false -> None
        assert_eq!(build_soi_hint_line("ok", true, false, 0, 10), None);
        // enabled=false -> None
        assert_eq!(build_soi_hint_line("ok", false, true, 0, 10), None);
        // empty summary -> None
        assert_eq!(build_soi_hint_line("", true, true, 0, 10), None);
    }

    #[test]
    fn test_soi_hint_line_min_lines_threshold() {
        // raw_line_count (15) < min_lines (20) -> None
        assert_eq!(build_soi_hint_line("ok", true, true, 20, 15), None);
        // raw_line_count (15) == min_lines (15) -> Some
        assert!(build_soi_hint_line("ok", true, true, 15, 15).is_some());
        // raw_line_count (25) > min_lines (20) -> Some
        assert!(build_soi_hint_line("ok", true, true, 20, 25).is_some());
        // min_lines=0 always passes
        assert!(build_soi_hint_line("ok", true, true, 0, 0).is_some());
    }

    #[test]
    fn soi_hint_line_truncates_long_summary() {
        let long = "x".repeat(300);
        let result = build_soi_hint_line(&long, true, true, 0, 100).unwrap();
        // Should contain truncated text with "..." suffix
        assert!(result.contains("..."));
        // The raw summary portion should be at most 200 chars
        let soi_prefix = "[glass-soi] ";
        let start = result.find(soi_prefix).unwrap() + soi_prefix.len();
        let end = result.find("...").unwrap() + 3;
        assert!(end - start <= 200);
    }

    #[test]
    fn soi_hint_line_short_summary_not_truncated() {
        let short = "Build succeeded";
        let result = build_soi_hint_line(short, true, true, 0, 100).unwrap();
        assert!(result.contains(short));
        assert!(!result.contains("..."));
    }

    #[test]
    fn state_transition_command_start_from_prompt_active() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        assert_eq!(bm.blocks()[0].state, BlockState::PromptActive);
        bm.handle_event(&OscEvent::CommandStart, 1);
        assert_eq!(bm.blocks()[0].state, BlockState::InputActive);
    }

    #[test]
    fn out_of_order_finished_before_executed() {
        // CommandFinished without CommandExecuted should still complete
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        // Skip CommandExecuted, go straight to Finished
        bm.handle_event(&OscEvent::CommandFinished { exit_code: Some(0) }, 5);
        let block = &bm.blocks()[0];
        assert_eq!(block.state, BlockState::Complete);
        assert_eq!(block.exit_code, Some(0));
        // started_at was never set, so duration is None
        assert!(block.duration().is_none());
    }

    #[test]
    fn out_of_order_executed_without_command_start() {
        // CommandExecuted without CommandStart — block stays PromptActive→Executing
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandExecuted, 2);
        assert_eq!(bm.blocks()[0].state, BlockState::Executing);
    }

    #[test]
    fn rapid_full_lifecycle_under_1ms() {
        let mut bm = BlockManager::new();
        // All events at the same line — simulates sub-millisecond delivery
        bm.handle_event(&OscEvent::PromptStart, 100);
        bm.handle_event(&OscEvent::CommandStart, 100);
        bm.handle_event(&OscEvent::CommandExecuted, 100);
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(42),
            },
            100,
        );
        let block = &bm.blocks()[0];
        assert_eq!(block.state, BlockState::Complete);
        assert_eq!(block.exit_code, Some(42));
        assert!(block.started_at.is_some());
        assert!(block.finished_at.is_some());
    }

    #[test]
    fn negative_exit_code_stored() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 2);
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(-1),
            },
            3,
        );
        assert_eq!(bm.blocks()[0].exit_code, Some(-1));
    }

    #[test]
    fn large_exit_code_stored() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 2);
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(i32::MAX),
            },
            3,
        );
        assert_eq!(bm.blocks()[0].exit_code, Some(i32::MAX));
    }

    #[test]
    fn signal_exit_code_137_stored() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 2);
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(137),
            },
            3,
        );
        // 137 = 128 + SIGKILL(9)
        assert_eq!(bm.blocks()[0].exit_code, Some(137));
    }

    #[test]
    fn visible_blocks_saturating_add_no_overflow() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        // Should not panic on near-max values
        let result = bm.visible_blocks(usize::MAX - 10, 100);
        // Block at line 0 is below the viewport
        assert!(result.is_empty());
    }

    #[test]
    fn block_pruning_at_max_capacity() {
        let mut bm = BlockManager::new();
        // Fill to MAX_BLOCKS
        for i in 0..MAX_BLOCKS {
            bm.handle_event(&OscEvent::PromptStart, i);
        }
        assert_eq!(bm.blocks().len(), MAX_BLOCKS);
        // One more triggers pruning (removes 10%)
        bm.handle_event(&OscEvent::PromptStart, MAX_BLOCKS);
        assert!(bm.blocks().len() < MAX_BLOCKS);
        assert_eq!(bm.blocks().len(), MAX_BLOCKS - MAX_BLOCKS / 10 + 1);
        // Current block is valid
        assert!(bm.current_block_mut().is_some());
    }
}
