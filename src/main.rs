use std::fs;
use std::process;
use std::str;
use std::time::{ Duration, Instant, UNIX_EPOCH };
use std::collections::{ HashMap };

use serde::{ Deserialize, Serialize };

use console::{ style, Emoji };

use indicatif::{ ProgressBar, ProgressStyle, HumanDuration };

use clap::Parser;

static GREEN_TICK: Emoji<'_, '_> = Emoji("✅", "");
static RED_CROSS: Emoji<'_, '_> = Emoji("❌", "");

#[derive(Serialize, Deserialize)]
struct Command {
    command: String,
    arguments: Vec<String>,
    run_if: Option<Vec<String>>
}

#[derive(Serialize, Deserialize)]
struct Executable {
    target: String,
    commands: Vec<Command>
}

#[derive(Serialize, Deserialize)]
struct CoyoteJson {
    project_name: String,
    variables: serde_json::Value,
    executables: Vec<Executable>
}

#[derive(Serialize, Deserialize)]
struct CoyoteLock {
    last_modified: HashMap<String, String>,

    #[serde(skip_serializing, skip_deserializing)]
    rebuild: bool
}

#[derive(Parser)]
struct Cli {
    /// Recipe for coyote to build
    recipe: Option<String>,

    /// Rebuilds the entire recipe regardless of coyote.LOCK
    /// (ignores `run_if` etc.)
    #[arg(short, long, default_value_t = false)]
    rebuild: bool
}

fn format_error(message: &str, fatal: bool, subname: &str) {
    let mut msg: String = String::new();
    if subname.is_empty() {
        msg = format!("[{}] ", style("coyote").red());
    } else {
        msg = format!("[{}/{}] ",
            style("coyote").red(),
            style(subname).color256(8)
        );
    }
    msg += &message;

    if fatal {
        msg += format!(" ({})", style("fatal").red().bright()).as_str();
    }

    eprintln!("{}", msg);
    process::exit(-1);
}

fn execute_command_opt(
    command: Option<Vec<String>>,
    command_string: &String) -> String {
    let mut cmd = match command {
        Some(c) => c,
        None => {
            format_error(
                format!("Failed to parse command '{}'", command_string)
                    .as_str(),
                true,
                "preprocessor"
            );
            eprintln!("[coyote/preprocessor] Failed to parse command '{}'",
                command_string);
            process::exit(-1);
        }
    };

    let mut cmd_process = process::Command::new(cmd[0].clone());
    cmd_process.args(&mut cmd[1..]);

    if let Ok(output) = cmd_process.output() {
        // check if output is damaged
        if !output.status.success() {
            // convert stderr to string
            let s = match str::from_utf8(&output.stderr) {
                Ok(v) => v,
                Err(_) => process::exit(-1)
            }.to_owned();
            format_error(
                format!("Failed to execute command '{}':\n\n{}",
                    command_string,
                    s
                ).as_str(),
                true,
                "preprocessor"
            );
            eprintln!(
                "[coyote/preprocessor] Failed to execute command '{}':\n\n{}",
                command_string,
                s
            );
            process::exit(-1);
        }

        // convert stdout into a string, and pass that as the command output
        let out = match str::from_utf8(&output.stdout) {
            Ok(v) => v,
            Err(_) => process::exit(-1)
        }.to_owned();

        return out;
    } else {
        format_error(
            format!("Failed to execute command '{}'", command_string).as_str(),
            true,
            "preprocessor"
        );
        "".to_string()
    }
}

fn patch_variable_references(value: &String,
    variables: &HashMap<String, String>) -> Result<String, String> {
    let mut tokens: String = String::new();
    let mut var_data: String = String::new();
    let mut var_found = false;

    for c in value.chars() {
        if var_found {
            if c == '}' {
                // variable ended
                let var_ref = tokens.replace("{", "");
                match variables.get(&var_ref) {
                    Some(value) => var_data += value,
                    None => return Err(var_ref)
                }

                var_found = false;
            } else if c == '{' {
                // escape
                var_found = false;
                var_data.push('{');
            } else {
                tokens.push(c);
            }
        } else if c == '{' {
            var_found = true;
            tokens = "{".to_string();
        } else {
            var_data.push(c);
        }
    }

    Ok(var_data)
}

fn patch_string(value: &String, variables: &HashMap<String, String>) ->
    Result<String, String>
{
    let mut tokens: String = String::new();
    let mut var_data: String = String::new();
    let mut var_found = false;
    let mut cmd_found = false;


    for c in value.chars() {
        if var_found {
            if c == '}' {
                // variable ended
                let var_ref = tokens.replace("{", "");
                match variables.get(&var_ref) {
                    Some(value) => var_data += value,
                    None => return Err(var_ref)
                }

                var_found = false;
            } else if c == '{' {
                // escape
                var_found = false;
                var_data.push('{');
            } else {
                tokens.push(c);
            }
        } else if cmd_found {
            if c == '`' {
                // command ended
                let replace_cmd = tokens.replace("`", "");
                let trimmed_cmd = replace_cmd.trim();
                let cmd = shlex::split(trimmed_cmd);

                var_data += &execute_command_opt(cmd.clone(), &replace_cmd);
            } else {
                tokens.push(c);
            }
        } else if c == '{' {
            var_found = true;
            tokens = "{".to_string();
        } else if c == '`' {
            cmd_found = true;
            tokens = "`".to_string();
        } else {
            var_data.push(c);
        }
    }

    Ok(var_data)
}

fn check_var_string(string: Result<String, String>, key: String) -> String {
    match string {
        Ok(value) => value,
        Err(reference) => {
            format_error(format!("'{}' references '{}' which is not defined",
                key,
                reference).as_str(),
                true,
                "preprocessor");
            process::exit(-1);
        }
    }
}

fn get_file_modified_time(path: String) -> u64 {
    if let Ok(meta) = fs::metadata(path.as_str()) {
        meta.modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    } else {
        format_error(format!("Cannot read or open metadata of file '{}'", path)
            .as_str(),
            false,
            ""
        );
        0u64
    }
}

fn condition_met(cond: &Vec<String>, target: String, lock: &mut CoyoteLock)
    -> bool {
    if cond.len() == 0 {
        format_error(format!(
            "No condition specifier for 'run_if' in target '{}'", target)
            .as_str(),
            true,
            "run_if"
        );
    }
    match cond[0].as_str() {
        "modified" => {
            if cond.len() > 2 {
                format_error(format!("Condition 'modified' in target '{}' must \
                    have 1 argument: <path>", target).as_str(), true, "run_if");
            }
            // test the file's metadata against the build directory
            let file_modified_time = get_file_modified_time(cond[1].clone());
            let last_modified = match lock.last_modified.get(&cond[1]) {
                Some(child) => {
                    match child.parse::<u64>() {
                        Ok(v) => v,
                        Err(_) => {
                            format_error(format!("Failed to parse condition '{}\
                                'to u64 in target {}", child, target).as_str(),
                                true, "run_if"
                            );
                            process::exit(-1);
                        }
                    }
                },
                None => {
                    lock.last_modified.insert(
                        cond[1].clone(),
                        file_modified_time.to_string()
                    );
                    return true;
                }
            };

            *lock
                .last_modified
                .get_mut(&cond[1])
                .unwrap() = file_modified_time.to_string();

            last_modified != file_modified_time
        }
        _ => {
            format_error(
                format!("Unknown condition type '{}' in target '{}'",
                    cond[0],
                    target)
                .as_str(),
                true,
                "run_if"
            );
            false
        }
    }
}

impl CoyoteLock {
    fn new() -> Self {
        CoyoteLock {
            last_modified: HashMap::new(),
            rebuild: false
        }
    }
}

impl CoyoteJson {
    fn preprocess(&mut self) {
        // firstly, preprocess all of the variable declarations (eg. inserting
        // variable references where $<name> is present, etc.)
        let mut variables: HashMap<String, String> = HashMap::new();

        for (k, v) in self.variables.as_object().unwrap() {
            let key = k.as_str().to_string();
            let value = v.as_str().unwrap().to_string();

            let patched = patch_string(&value, &variables);
            variables.insert(key.clone(), check_var_string(patched, key));
        }

        // go through all commands and fill in all strings with preprocessing
        // data
        for exec in &mut self.executables {
            for command in &mut exec.commands {
                let processed = check_var_string(patch_variable_references(
                    &command.command,
                    &variables
                ), command.command.clone());
                command.command = processed;

                let mut modified_arguments: Vec<String> = Vec::new();

                // loop through arguments and patch them
                for argument in &mut command.arguments {
                    let processed = check_var_string(patch_variable_references(
                        &argument,
                        &variables
                    ), argument.clone());

                    modified_arguments.push(processed);
                }

                command.arguments = modified_arguments;

                // finally, loop through all of the run_ifs and patch them
                if let Some(ref runifs) = &command.run_if {
                    let mut modified_runif: Vec<String> = Vec::new();

                    for argument in runifs.into_iter() {
                        let processed = check_var_string(
                            patch_variable_references(
                                &argument,
                                &variables
                            ),
                            argument.clone()
                        );

                        modified_runif.push(processed);
                    }

                    command.run_if = Some(modified_runif);
                }
            }
        }
    }
}

impl Command {
    fn to_string(&self) -> String {
        format!("{} {}", self.command, self.arguments.join(" "))
    }
}

impl Executable {
    fn build(&self, lock: &mut CoyoteLock) {
        let mut index = 1;

        for command in &self.commands {
            // firstly, check if the run_if condition is set and whether or not
            // it is met
            if let Some(condition) = &command.run_if {
                if !lock.rebuild {
                    if !condition_met(condition, self.target.clone(), lock) {
                        // if the condition is not met, skip this compilation
                        // step
                        continue;
                    }
                }
            }

            let mut cmd = process::Command::new(command.command.clone());
            cmd.args(command.arguments.clone());

            // setup spinner for current command
            let spinner_style =
                ProgressStyle::with_template(
                    "{prefix:.bold.dim} {spinner} {wide_msg}"
                )
                .unwrap()
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

            let pb = ProgressBar::new_spinner();

            pb.set_style(spinner_style);
            pb.enable_steady_tick(Duration::from_millis(75));
            pb.set_message(command.to_string());
            pb.set_prefix(format!("   {} ->",
                style(
                    format!("({}/{})", index, self.commands.len())
                ).color256(8)
            ));

            if let Ok(output) = cmd.output() {
                let mut finish_emoji = GREEN_TICK;
                if !output.status.success() {
                    // convert stderr to string
                    let s = match str::from_utf8(&output.stderr) {
                        Ok(v) => v,
                        Err(_) => process::exit(-1)
                    }.to_owned();

                    format_error(
                        format!("Failed to execute command '{}': \n\n{}",
                        command.command, s).as_str(),
                        false,
                        ""
                    );
                    finish_emoji = RED_CROSS;
                }

                // set finish message
                pb.set_prefix("");
                pb.finish_with_message(
                    format!("{} {} {}",
                        finish_emoji,
                        style("Finished").blue(),
                        command.to_string()
                    )
                );
                pb.finish();
            } else {
                format_error(format!("Failed to execute command '{}'",
                    command.command).as_str(),
                    true,
                    ""
                );
            }

            index += 1;
        }
    }
}

fn main() {
    let arguments = Cli::parse();
    let mut contents = String::new();

    // if there is a recipe present, use that JSON file instead of the default.
    // NOTE: All recipes operate on one coyote.LOCK file
    if let Some(recipe) = arguments.recipe {
        contents = match fs::read_to_string(
            "./coyote-".to_string() + &recipe + ".json") {
            Ok(x) => x,
            Err(_) => {
                format_error(format!(
                    "Couldn't find file for recipe '{}' (note - recipe JSON fi\
                    les must be prefixed with 'coyote-' to be recognised)",
                    recipe).as_str(), true, "recipe"
                );
                process::exit(-1);
            }
        };

        println!("{}", style(format!("[coyote] Building recipe '{}'", recipe))
            .green());
    } else {
        contents = match fs::read_to_string("./coyote.json") {
            Ok(x) => x,
            Err(_) => {
                format_error(
                    "Directory does not contain `coyote.json`",
                    true,
                    ""
                );
                process::exit(-1);
            }
        };
    }

    // convert the coyote.json file into a struct with serde
    let build_result: Result<CoyoteJson, serde_json::Error> =
        serde_json::from_str(&contents);

    let mut build_info: CoyoteJson = match build_result {
        Ok(x) => x,
        Err(error) => {
            format_error(format!("Malformed 'coyote.json' detected: {}",
                error).as_str(), true, "");
            process::exit(-1);
        }
    };

    // open coyote.LOCK if it exists, and if it does not exist then create a
    // new one
    let lock_contents = match fs::read_to_string("./coyote.LOCK") {
        Ok(x) => x,
        Err(_) => {
            // file does not exist
            if let Ok(_) = fs::File::create("./coyote.LOCK") {
                "".to_string()
            } else {
                format_error("Failed to create 'coyote.LOCK", true, "");
                process::exit(-1);
            }
        }

    };

    let lock_result: Result<CoyoteLock, serde_json::Error> =
        serde_json::from_str(&lock_contents);

    let mut lockfile: CoyoteLock = match lock_result {
        Ok(x) => x,
        Err(x) => {
            if !lock_contents.is_empty() {
                format_error(format!("Malformed 'coyote.LOCK' detected: {}",
                    x).as_str(), true, "");
            }
            CoyoteLock::new()
        }
    };

    lockfile.rebuild = arguments.rebuild;

    // preprocess the build information
    build_info.preprocess();

    // get the current time (to calculate the elapsed time after build finishes)
    let started = Instant::now();

    // loop through all of the executables and build them in order
    let mut exec_index = 1;
    for executable in &build_info.executables {
        println!("[{}/{}] {} '{}'",
            exec_index,
            build_info.executables.len(),
            style("Building target").cyan(),
            executable.target
        );

        executable.build(&mut lockfile);

        exec_index += 1;
    }

    // overwrite coyote.LOCK
    if let Ok(lock_json) = serde_json::to_string(&lockfile) {
        fs::write("./coyote.LOCK", lock_json).expect("Uh oh");
    }
    else {
        format_error("Failed to convert coyote.LOCK into JSON format.",
            true,
            ""
        );
    }

    println!("{}", style(format!(
        "[coyote] Finished building project '{}' in {}",
        build_info.project_name,
        HumanDuration(started.elapsed()))).green());
}
