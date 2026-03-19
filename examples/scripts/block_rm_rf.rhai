// Block rm -rf -- warn when a dangerous rm pattern is detected.
// Hook: command_start
//
// Install: copy both files to ~/.glass/scripts/ or <project>/.glass/scripts/

let cmd = event.command;

let dangerous = [
    "rm -rf /",
    "rm -rf /*",
    "rm -rf ~",
    "rm -rf $HOME",
];

for pattern in dangerous {
    if cmd.contains(pattern) {
        glass.log("warn", `Dangerous command detected: ${cmd}`);
    }
}
