use clap::Parser;
use log::info;
use side_channel_crackme_solver::args::Args;
use side_channel_crackme_solver::command::{PreparedCommand, InputPreparer};
use side_channel_crackme_solver::misc;
use side_channel_crackme_solver::workers::ThreadsData;
use side_channel_crackme_solver::workers;
use std::error::Error;
use std::path::Path;
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = Args::parse();

    if !Path::new(&args.exe_path).is_file() {
        return Err(format!("File does not exist: {}", args.exe_path).into());
    }

    if which::which("perf").is_err() {
        return Err("Can't find perf binary in your $PATH".into());
    }

    // Logging turned on by default cuz usually
    // I want to actually see what the program is doing
    if !args.quiet {
        let default = env_logger::Env::default()
            .filter_or(env_logger::DEFAULT_FILTER_ENV, "info");
        env_logger::init_from_env(default);
    }

    if args.alphabet.is_empty() {
        args.alphabet = (1..0x80).map(|n| char::from(n)).collect();
        info!("No alphabet given. Using the default one: ascii values from 0x01 to 0x7f.");
    }
    
    if args.threads == 0 {
        args.threads = thread::available_parallelism().unwrap().get();
        info!("No number of threads given. Using the detected recommended number instead: {}",
              args.threads);
    }
    
    main_loop(args);

    Ok(())
}

pub fn main_loop(mut args: Args) {
    let data = Arc::new(Mutex::new(ThreadsData::new()));
    {
        let mut data = data.lock().unwrap();
        data.chars_to_process = args.alphabet.chars().collect();
    }

    if args.length == 0 {
        let input_preparer = InputPreparer::new(
            args.input_beg.clone(),
            args.input_end.clone(),
            args.length,
            args.padding,
        );
        let prepared_command = PreparedCommand::new(
            &args.exe_path,
            "instructions",
            args.iterations,
            args.stdin
        );

        info!("No length found. Searching for length...");
        args.length = misc::find_length(args.max_length, &input_preparer, &prepared_command);
        info!("Found length: {}. Proceed with caution, the length might be wrong.", args.length);
    }

    info!("Starting solver...");
    let input_preparer = InputPreparer::new(
        args.input_beg.clone(),
        args.input_end.clone(),
        args.length,
        args.padding,
    );
    let prepared_command = PreparedCommand::new(
        &args.exe_path,
        &args.event,
        args.iterations,
        args.stdin
    );
    let mut thread_workers = vec![];
    for _ in 0..args.threads {
        let data = Arc::clone(&data);
        let prepared_command = prepared_command.clone();
        let input_preparer = input_preparer.clone();
        thread_workers.push(thread::spawn(
                move || workers::thread_worker(data, prepared_command, input_preparer)
        ));
    }

    loop {
        // Wait till there are no chars left to process
        loop {
            let chars_left;
            {
                let data = data.lock().unwrap();
                chars_left = data.chars_to_process.len();
            }

            if chars_left > 0 {
                thread::sleep(time::Duration::from_millis(100));
                continue;
            } else {
                break;
            }
        }

        {
            let mut data = data.lock().unwrap();
            if process_found_chars(&args, &mut data) {
                break;
            }
        }
    }

    // Final results
    {
        let data = data.lock().unwrap();
        if !args.quiet {
            println!("Found: {}", data.found_password_prefix);
        } else {
            print!("{}", data.found_password_prefix);
        }
    }
    for thread in thread_workers {
        thread.join().unwrap();
    }
}

fn process_found_chars(args: &Args, data: &mut ThreadsData) -> bool {
    data.processed_chars.sort();
    let &(_, char) = data.processed_chars.last().unwrap();
    data.found_password_prefix.push(char);

    // Confirm starts_with and ends_with
    if !args.starts_with.is_empty() {
        let compare_to = std::cmp::min(
            data.found_password_prefix.len(),
            args.starts_with.len()
        );
        if data.found_password_prefix[..compare_to] != args.starts_with[..compare_to] &&
                !args.quiet {
            println!("Found password and starts_with argument don't match-up");
            println!("Found password: {}", data.found_password_prefix);
            println!("starts_with: {}", args.starts_with);
            println!("Ending execution...");
            process::exit(-1);
        }
    }

    if !args.ends_with.is_empty() {
        let end_start_idx = args.length - args.ends_with.len();
        if data.found_password_prefix.len() > end_start_idx {
            let postfix = &data.found_password_prefix[end_start_idx..];
            let ends_with = &args.ends_with[..postfix.len()];
            if postfix != ends_with && !args.quiet {
                println!("Found password and ends_with argument don't match-up");
                println!("Found password: {}", data.found_password_prefix);
                println!("ends_with: {}", args.ends_with);
                println!("Ending execution...");
                process::exit(-1);
            }
        }
    }

    // If password length is satisfied then quit.
    if data.found_password_prefix.len() == args.length {
        return true;
    }

    data.processed_chars = Vec::new();
    data.chars_to_process = args.alphabet.chars().collect();

    if !args.quiet {
        info!("Currently found password: {}", data.found_password_prefix);
    }

    false
}
