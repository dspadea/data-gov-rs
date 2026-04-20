use data_gov::{DataGovClient, ckan::models::Package};

use super::{
    color_blue, color_blue_bold, color_bold, color_dimmed, color_green, color_green_bold,
    color_yellow, color_yellow_bold,
};

/// Print package details (shared between REPL and CLI modes)
pub fn print_package_details(package: &Package) {
    println!("\n{}", color_blue_bold("📦 Dataset Details"));
    println!("{}: {}", color_bold("Name"), color_yellow(&package.name));

    if let Some(title) = &package.title {
        println!("{}: {}", color_bold("Title"), title);
    }

    if let Some(notes) = &package.notes {
        println!("\n{}: ", color_bold("Description"));
        println!("{}", color_dimmed(notes));
    }

    if let Some(license_title) = &package.license_title {
        println!(
            "\n{}: {}",
            color_bold("License"),
            color_green(license_title)
        );
    }

    if let Some(author) = &package.author {
        println!("{}: {}", color_bold("Author"), author);
    }

    if let Some(maintainer) = &package.maintainer {
        println!("{}: {}", color_bold("Maintainer"), maintainer);
    }

    // Display resources
    let resources = DataGovClient::get_downloadable_resources(package);
    if !resources.is_empty() {
        println!(
            "\n{} {} downloadable resources:",
            color_bold("📁"),
            resources.len()
        );

        for (i, resource) in resources.iter().enumerate() {
            let name = resource.name.as_deref().unwrap_or("Unnamed");
            let format = resource.format.as_deref().unwrap_or("Unknown");
            let size = resource
                .size
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();

            println!(
                "  {}. {} {} {}",
                color_blue_bold(&i.to_string()),
                color_yellow(name),
                color_green(&format!("[{}]", format)),
                color_dimmed(&size)
            );

            if let Some(desc) = &resource.description
                && !desc.is_empty()
            {
                let truncated = if desc.chars().count() > 80 {
                    let s: String = desc.chars().take(80).collect();
                    format!("{s}...")
                } else {
                    desc.clone()
                };
                println!("     {}", color_dimmed(&truncated));
            }
        }

        println!(
            "\n{} Use 'data-gov download {}' to download all resources",
            color_bold("💡"),
            color_yellow(&package.name)
        );
        println!(
            "{} Use 'data-gov download {} <index|name>' to download by index or name",
            color_bold("💡"),
            color_yellow(&package.name)
        );
    } else {
        println!("\n{} No downloadable resources found", color_yellow("⚠️"));
    }

    println!();
}

/// Print help for CLI mode
pub fn print_cli_help() {
    println!("\n{}", color_blue_bold("📚 CLI Mode Commands"));
    println!();

    let commands = vec![
        (
            "search <query> [limit]",
            "Search for datasets (filtered by active org)",
            "search \"climate data\" 20",
        ),
        (
            "show [dataset_id]",
            "Show dataset info (uses active dataset if omitted)",
            "show electric-vehicle-population-data",
        ),
        (
            "download [dataset] [selectors...]",
            "Download resources (by index or name)",
            "download electric-vehicle-population-data 0",
        ),
        (
            "cd <path>",
            "Navigate org/dataset context (like cd)",
            "cd nasa-gov",
        ),
        (
            "list organizations",
            "List government organizations",
            "list organizations",
        ),
        ("info", "Show client and session information", "info"),
    ];

    for (cmd, desc, example) in commands {
        println!("{:30} {}", color_green_bold(cmd), desc);
        println!(
            "{:30} {}: data-gov {}",
            "",
            color_dimmed("Example"),
            color_blue(example)
        );
        println!();
    }

    println!("{}", color_yellow_bold("💡 Interactive Mode:"));
    println!(
        "  Run without arguments to start interactive REPL: {}",
        color_blue("data-gov")
    );
    println!();
}

/// Print help for REPL mode
pub fn print_repl_help() {
    println!("\n{}", color_blue_bold("📚 Available Commands"));
    println!();

    let commands = vec![
        (
            "search <query> [limit]",
            "Search datasets (filtered by active org)",
            "search climate data 20",
        ),
        (
            "show [dataset_id]",
            "Show dataset info (uses active dataset)",
            "show electric-vehicle-population-data",
        ),
        (
            "download [dataset] [selectors...]",
            "Download resources (by index or name)",
            "download electric-vehicle-population-data 0",
        ),
        ("cd <path>", "Navigate org/dataset context", "cd epa-gov"),
        (
            "list organizations",
            "List government organizations",
            "list orgs",
        ),
        (
            "lcd <path>",
            "Set local download directory",
            "lcd ./downloads",
        ),
        ("info", "Show session and client info", "info"),
        ("help", "Show this help message", "help"),
        ("quit", "Exit the REPL", "quit"),
    ];

    for (cmd, desc, example) in commands {
        println!("{:25} {}", color_green_bold(cmd), desc);
        println!(
            "{:25} {}: {}",
            "",
            color_dimmed("Example"),
            color_blue(example)
        );
        println!();
    }

    println!("{}", color_yellow_bold("💡 Pro Tips:"));
    println!(
        "  • Use short commands: {} for search, {} for show, {} for download",
        color_green("s"),
        color_green("d"),
        color_green("dl")
    );
    println!(
        "  • Navigate like a filesystem: {}, {}, {}",
        color_blue("cd epa-gov"),
        color_blue("cd air-quality"),
        color_blue("cd ..")
    );
    println!(
        "  • Or jump directly: {}, {}, {}",
        color_blue("cd /epa-gov/air-quality"),
        color_blue("cd /nasa-gov"),
        color_blue("cd /")
    );
    println!(
        "  • Then just: {}, {}, {}",
        color_blue("show"),
        color_blue("download 0"),
        color_blue("search pollution")
    );
    println!(
        "  • Download multiple resources: {}",
        color_blue("download \"RDF File\" \"XML File\"")
    );
    println!(
        "  • Aliases: {} = {}, {} = {}",
        color_green("select"),
        color_green("cd"),
        color_green("setdir"),
        color_green("lcd")
    );
    println!("  • Downloads are organized by dataset name in subdirectories");
    println!(
        "  • Create scripts with {} for automation",
        color_blue("#!/usr/bin/env data-gov")
    );
    println!();
}
