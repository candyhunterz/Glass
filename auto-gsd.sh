#!/bin/bash
# auto-gsd.sh — Fully automated plan+execute loop for GSD phases
# Each step runs in a fresh claude context (no manual intervention needed)
# Usage: bash auto-gsd.sh [start_phase] [end_phase]

START_PHASE=${1:-53}
END_PHASE=${2:-60}

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " GSD Auto-Runner: Phases $START_PHASE → $END_PHASE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

for phase in $(seq "$START_PHASE" "$END_PHASE"); do
  echo ""
  echo "╔══════════════════════════════════════════════════════╗"
  echo "  PHASE $phase — PLANNING"
  echo "╚══════════════════════════════════════════════════════╝"

  claude -p "/gsd:plan-phase $phase"
  if [ $? -ne 0 ]; then
    echo "✗ Planning failed at phase $phase"
    exit 1
  fi
  echo "✓ Phase $phase planned"

  echo ""
  echo "╔══════════════════════════════════════════════════════╗"
  echo "  PHASE $phase — EXECUTING"
  echo "╚══════════════════════════════════════════════════════╝"

  claude -p "/gsd:execute-phase $phase --auto"
  if [ $? -ne 0 ]; then
    echo "✗ Execution failed at phase $phase"
    exit 1
  fi
  echo "✓ Phase $phase complete"
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " GSD Auto-Runner: All phases complete ($START_PHASE → $END_PHASE)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
