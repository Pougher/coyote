# Coyote Build System

## A build system written in Rust :crab: for general purpose development

Coyote is a build system that aims to be explicit and easy to work with, serving as a tool that makes writing project build systems alot easier (at the expense of scaling)

### Project structure
Lets say you were working on a project that builds the file `hello.c` into the executable `hello` - to package this into a coyote build JSON, all you would have to do is the following:

1) Define a project name

    Absolutely needed for any _serious_ project, so for this I am going with `hello` (I know, creative!)
    
    To do this, all you need is the following line of JSON: `"project_name": "hello"` - its as simple as that!
2) Variables
    
    In order to have a project, you have to have dependencies and aliases, lest your build files be too readable! This is easily doable in the `coyote.json` file, with the following line being used to mark the start of your variable list (its pretty self descriptive) `"variables": { ... }`

    For example, lets say you wanted to store the output filename for your project as a variable. This is easy to do with the following:
    ```json
    "variables": {
        "output": "hello"
    }
    ```
    Furthermore, if you want to reference a variable in another variable, all you have to do is place the reference variable name in a pair of `{}`. If you wish to use `{` for other purposes, you can also do that via the escape operator `{{`
    > Note: Variables are evaluated in alphanumerical order regardless of the order they are specified in.
3) Executables

    A `coyote.json` can specify multiple executable 'targets' that it can build one after another. Each 'target' also has a list of commands that it runs upon its execution. For our purposes though, we won't be needing multiple targets or multiple commands. A simple `gcc hello.c -o hello` will do for us.

    Each executable is specified in the `"executables": [ ... ]` block of the JSON and are specified as lists. Every executable is an object and is structured as the following:
    ```json
    "executables": [
        {
            "target": "hello",
            "commands": [
                { ... },
                { ... }
            ]
        }
    ]
    ```
4) Commands
    
    Every executable has a list of commands that run in specification order, defining one command and a list of arguments, for example the command to build `hello` might look a bit like this:
    ```json
    {
        "command": "gcc",
        "arguments": [ "hello.c", "-o{target}" ]
    }
    ```
    But wait! There's more!

    Commands may also optionally specify a `run_if` list, that serves as a single condition (along with some arguments) that specify to coyote whether or not a command should be run or not. For example, if you didn't want to waste time recompiling unmodified code, you could use a `modified` condition along with a filename, which looks like this:
    ```json
    {
        "command": "gcc",
        "arguments": [ "hello.c", "-o{target}" ],
        "run_if": [
            "modified",
            "hello.c"
        ]
    }
    ```
    This command would only be run if `main.c` is detected to be modified!

5) More on `run_if`

    As of now, there is only 1 `run_if` specifier - `modified`. All it does is check for the modification of a file.

6) Putting it all together
    
    Here is our finished `coyote.json` for building a single file with `gcc`:
    ```json
    {
        "project_name": "hello",
        "variables": {
            "target": "hello"
        },
        "executables": [
            {
                "target": "main",
                "commands": [
                    {
                        "command": "gcc",
                        "arguments": [ "hello.c", -O3", "-o{target}" ],
                        "run_if": [ "modified", "hello.c" ]
                    }
                ]
            }
        ]
    }

    ```

### Other stuff
Coyote also supports multiple 'recipes' that can be built using a singular command line argument. These work by loading a different `coyote.json` where the filename is formatted as follows `coyote-[recipe].json`

Coyote also supports the following command line options:

* `-r`, `--rebuild`: Ignores all `run_if` statements and builds the entire project from scratch
