use std::borrow::Borrow;

use anyhow::{Context, Result};
use clap::{Arg, ArgMatches, Command};

use symbolic::common::{ByteView, Language, Name, NameMangling};
use symbolic::debuginfo::{Function, Object};
use symbolic::demangle::{Demangle, DemangleOptions};

fn print_name<'a, N: Borrow<Name<'a>>>(name: Option<N>, matches: &ArgMatches) {
    match name.as_ref().map(Borrow::borrow) {
        None => print!("??"),
        Some(name) if name.as_str().is_empty() => print!("??"),
        Some(name) if matches.is_present("demangle") => {
            print!("{}", name.try_demangle(DemangleOptions::name_only()));
        }
        Some(name) => print!("{}", name),
    }
}

fn print_range(start: u64, len: Option<u64>, matches: &ArgMatches) {
    if matches.is_present("ranges") {
        print!(" ({:#x} - ", start);
        match len {
            Some(len) => print!("{:#x})", start + len),
            None => print!("??)"),
        }
    }
}

fn resolve(function: &Function<'_>, addr: u64, matches: &ArgMatches) -> Result<bool> {
    if function.address > addr || function.address + function.size <= addr {
        return Ok(false);
    }

    if matches.is_present("inlines") {
        for il in &function.inlinees {
            resolve(il, addr, matches)?;
        }
    }

    for line in &function.lines {
        if line.address + line.size.unwrap_or(1) <= addr {
            continue;
        } else if line.address > addr {
            break;
        }

        if matches.is_present("functions") {
            print_name(Some(&function.name), matches);
            print_range(function.address, Some(function.size), matches);
            print!("\n  at ");
        }

        let file = if matches.is_present("basenames") {
            line.file.name_str()
        } else {
            line.file.path_str().into()
        };

        print!("{}:{}", file, line.line);
        print_range(line.address, line.size, matches);
        println!();

        return Ok(true);
    }

    Ok(false)
}

fn execute(matches: &ArgMatches) -> Result<()> {
    let path = matches.value_of("path").unwrap_or("a.out");
    let view = ByteView::open(path).context("failed to open file")?;
    let object = Object::parse(&view).context("failed to parse file")?;
    let session = object.debug_session().context("failed to process file")?;
    let symbol_map = object.symbol_map();

    'addrs: for addr in matches.values_of("addrs").unwrap_or_default() {
        let addr = match addr.strip_prefix("0x") {
            Some(addr) => u64::from_str_radix(addr, 16),
            None => addr.parse(),
        }
        .context("unable to parse address")?;

        for function in session.functions() {
            let function = function.context("failed to read function")?;
            if resolve(&function, addr, matches)? {
                continue 'addrs;
            }
        }

        if matches.is_present("functions") {
            if let Some(symbol) = symbol_map.lookup(addr) {
                print_name(
                    symbol
                        .name
                        .as_ref()
                        .map(|n| Name::new(n.as_ref(), NameMangling::Mangled, Language::Unknown)),
                    matches,
                );
                print_range(symbol.address, Some(symbol.size), matches);
                print!("\n  at ");
            }
        }

        println!("??:0");
    }

    Ok(())
}

fn main() {
    let about = r#"addr2line translates addresses into file names and line numbers. Given an address in an executable or an offset in a section of a relocatable object, it uses the debugging information to figure out which file name and line number are associated with it.

addr2line has two modes of operation.

In the first, hexadecimal addresses are specified on the command line, and addr2line displays the file name and line number for each address.

In the second, addr2line reads hexadecimal addresses from standard input, and prints the file name and line number for each address on standard output. In this mode, addr2line may be used in a pipe to convert dynamically chosen addresses."#;
    let matches = Command::new("addr2line")
        .about(about)
        .arg(
            Arg::new("demangle")
                .short('C')
                .long("demangle")
                .help("Decode (demangle) low-level symbol names into user-level names. Besides removing any initial underscore prepended by the system, this makes C ++ function names readable.")
        )
        .arg(
            Arg::new("path")
                .short('e')
                .long("exe")
                .number_of_values(1)
                .help("Specify the name of the executable for which addresses should be translated. The default file is a.out.")
        )
        .arg(
            Arg::new("functions")
                .short('f')
                .long("functions")
                .help("Display function names as well as file and line number information."),
        )
        .arg(
            Arg::new("ranges")
                .short('r')
                .long("ranges")
                .help("Display function address ranges in addition to function names."),
        )
        .arg(
            Arg::new("basenames")
                .short('s')
                .long("basenames")
                .help("Display only the base of each file name."),
        )
        .arg(
            Arg::new("inlinees")
                .short('i')
                .long("inlinees")
                .help("If the address belongs to a function that was inlined, the source information for all enclosing scopes back to the first non-inlined function will also be printed. For example, if \"main\" inlines \"callee1\" which inlines \"callee2\", and address is from \"callee2\", the source information for \"callee1\" and \"main\" will also be printed.")
        )
        .arg(
            Arg::new("addrs")
                .required(true)
                .takes_value(true)
                .multiple_values(true)
                .help("Addresses to be translated."),
        )
        .get_matches();

    match execute(&matches) {
        Ok(()) => (),
        Err(e) => eprintln!("{:?}", e),
    };
}
