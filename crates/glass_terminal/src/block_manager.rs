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
        }
    }

    /// Calculate the duration of command execution.
    pub fn duration(&self) -> Option<Duration> {
        match (self.started_at, self.finished_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }
}

/// Manages the collection of command blocks.
pub struct BlockManager {
    blocks: Vec<Block>,
    current: Option<usize>,
}

impl BlockManager {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            current: None,
        }
    }

    /// Process an OSC event and update block state.
    pub fn handle_event(&mut self, event: &OscEvent, line: usize) {
        match event {
            OscEvent::PromptStart => {
                let block = Block::new(line);
                self.blocks.push(block);
                self.current = Some(self.blocks.len() - 1);
            }
            OscEvent::CommandStart => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
                        block.state = BlockState::InputActive;
                        block.command_start_line = line;
                    }
                }
            }
            OscEvent::CommandExecuted => {
                if let Some(idx) = self.current {
                    if let Some(block) = self.blocks.get_mut(idx) {
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
                        block.state = BlockState::Complete;
                        block.exit_code = *exit_code;
                        block.output_end_line = Some(line);
                        block.finished_at = Some(Instant::now());
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
            OscEvent::PipelineStage { index, total_bytes, temp_path } => {
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
    pub fn visible_blocks(
        &self,
        display_offset: usize,
        screen_lines: usize,
    ) -> Vec<&Block> {
        let viewport_start = display_offset;
        let viewport_end = display_offset + screen_lines;

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

    /// Get all blocks.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Get a mutable reference to the current (most recent) block.
    pub fn current_block_mut(&mut self) -> Option<&mut Block> {
        self.current.and_then(|idx| self.blocks.get_mut(idx))
    }

    /// Find a block by its started_epoch and return a mutable reference.
    pub fn find_block_by_epoch_mut(&mut self, epoch: i64) -> Option<&mut Block> {
        self.blocks.iter_mut().rev().find(|b| b.started_epoch == Some(epoch))
    }
}

impl Default for BlockManager {
    fn default() -> Self {
        Self::new()
    }
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
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(0),
            },
            5,
        );
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
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(0),
            },
            5,
        );
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
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(0),
            },
            5,
        );
        // Block at lines 10-15
        bm.handle_event(&OscEvent::PromptStart, 10);
        bm.handle_event(&OscEvent::CommandStart, 11);
        bm.handle_event(&OscEvent::CommandExecuted, 11);
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(0),
            },
            15,
        );

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
        bm.handle_event(
            &OscEvent::CommandFinished {
                exit_code: Some(1),
            },
            0,
        );
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
        assert_eq!(
            format_duration(Duration::from_secs_f64(1.23)),
            "1.2s"
        );
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
    }

    #[test]
    fn format_duration_sub_millisecond() {
        assert_eq!(
            format_duration(Duration::from_secs_f64(0.0005)),
            "<1ms"
        );
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
        bm.handle_event(&OscEvent::PipelineStage {
            index: 0,
            total_bytes: 1024,
            temp_path: "/tmp/glass/stage_0".to_string(),
        }, 2);
        assert_eq!(bm.blocks()[0].pipeline_stages.len(), 1);
        assert_eq!(bm.blocks()[0].pipeline_stages[0].index, 0);
        assert_eq!(bm.blocks()[0].pipeline_stages[0].total_bytes, 1024);
        assert_eq!(bm.blocks()[0].pipeline_stages[0].temp_path, Some("/tmp/glass/stage_0".to_string()));
    }

    #[test]
    fn multiple_pipeline_stages_accumulate() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 3 }, 1);
        for i in 0..3 {
            bm.handle_event(&OscEvent::PipelineStage {
                index: i,
                total_bytes: 100 * (i + 1),
                temp_path: format!("/tmp/glass/stage_{}", i),
            }, 2);
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
        bm.handle_event(&OscEvent::PipelineStage {
            index: 0,
            total_bytes: 512,
            temp_path: "/tmp/glass/stage_0".to_string(),
        }, 1);
        assert!(bm.blocks().is_empty());
    }

    #[test]
    fn new_prompt_resets_pipeline_state() {
        let mut bm = BlockManager::new();
        bm.handle_event(&OscEvent::PromptStart, 0);
        bm.handle_event(&OscEvent::CommandStart, 1);
        bm.handle_event(&OscEvent::CommandExecuted, 1);
        bm.handle_event(&OscEvent::PipelineStart { stage_count: 2 }, 1);
        bm.handle_event(&OscEvent::PipelineStage {
            index: 0,
            total_bytes: 100,
            temp_path: "/tmp/glass/stage_0".to_string(),
        }, 2);
        // New prompt creates fresh block
        bm.handle_event(&OscEvent::PromptStart, 10);
        assert!(bm.blocks()[1].pipeline_stages.is_empty());
        assert_eq!(bm.blocks()[1].pipeline_stage_count, None);
        // First block still has its pipeline data
        assert_eq!(bm.blocks()[0].pipeline_stages.len(), 1);
    }
}
