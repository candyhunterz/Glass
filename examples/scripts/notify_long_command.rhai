// Notify Long Command -- log when a command exceeds a duration threshold.
// Hook: command_complete
//
// Install: copy both files to ~/.glass/scripts/ or <project>/.glass/scripts/

let duration_secs = event.duration_ms / 1000;
let threshold = 10;

if duration_secs >= threshold {
    glass.notify(`Command finished in ${duration_secs}s: ${event.command}`);
}
