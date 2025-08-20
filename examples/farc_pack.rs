use clap::Parser;
use farc::*;
use std::io::Write;

#[derive(Parser)]
struct Args {
	file: String,
	#[arg(short, long, default_value_t = true)]
	compress: bool,
}

fn main() {
	let args = Args::parse();

	let path = std::path::Path::new(&args.file);
	if !path.exists() {
		return;
	}

	if path.is_file() && path.extension().unwrap().to_str().unwrap() == "farc" {
		let farc = Farc::from_file(path).unwrap();
		let dir = path.with_extension("");
		std::fs::create_dir(&dir).unwrap();
		for (name, data) in farc.entries {
			let mut file = std::fs::OpenOptions::new()
				.write(true)
				.truncate(true)
				.create(true)
				.open(dir.join(name))
				.unwrap();
			file.write_all(&data.data.to_buf().unwrap()).unwrap();
			file.sync_all().unwrap();

			if let Some(modified_time) = data.modified_time {
				let time = std::time::SystemTime::UNIX_EPOCH
					+ std::time::Duration::from_secs(modified_time as u64);
				file.set_modified(time).unwrap()
			}
		}
	} else if path.is_dir() {
		let mut farc = Farc::new();
		for file in std::fs::read_dir(path).unwrap() {
			let path = file.unwrap().path();
			if path.is_dir() {
				continue;
			}
			let data = std::fs::read(&path).unwrap();
			farc.insert(path.file_name().unwrap().to_str().unwrap(), &data, None);
		}
		let file = path.with_extension("farc");
		farc.write_file(&file, args.compress).unwrap();
	} else {
		panic!("Must be farc file or directory")
	}
}
