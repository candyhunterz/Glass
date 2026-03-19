// Auto Git Status -- run `git status --short` after commands that touch tracked files.
// Hook: command_complete
//
// Install: copy both files to ~/.glass/scripts/ or <project>/.glass/scripts/

let cmd = event.command;

// Only trigger for commands likely to modify files
let dominated = false;
let triggers = ["git", "rm", "mv", "cp", "touch", "sed", "mkdir", "cargo"];
for t in triggers {
    if cmd.contains(t) {
        dominated = true;
    }
}

if dominated {
    glass.log("info", `Auto git status after: ${cmd}`);
}
