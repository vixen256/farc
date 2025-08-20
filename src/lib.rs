use binary_parser::*;
use libflate::gzip;
use std::collections::*;
use std::fs::File;
use std::io::{Cursor, Read, SeekFrom, Write};
use std::path::Path;

pub struct FarcEntry<'a> {
	pub data: BinaryParser<'a>,
	pub modified_time: Option<u32>,
}

pub struct Farc<'a> {
	pub entries: BTreeMap<String, FarcEntry<'a>>,
	pub alignment: u32,
}

impl<'a, 'b> Farc<'a> {
	pub fn new() -> Self {
		Self {
			entries: BTreeMap::new(),
			alignment: 16,
		}
	}

	pub fn insert(&mut self, name: &str, data: &[u8], modified_time: Option<u32>) {
		self.entries.insert(
			String::from(name),
			FarcEntry {
				data: BinaryParser::from_buf(data),
				modified_time: modified_time,
			},
		);
	}

	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
		let mut reader = BinaryParser::from_file(path)?;
		Self::from_parser(&mut reader)
	}

	pub fn from_parser(reader: &mut BinaryParser) -> Result<Self> {
		reader.set_big_endian(true);
		let signature = reader.read_string(4)?;
		let header_size = reader.read_u32()? + 8;

		let alignment = reader.read_u32()?;
		let mut entries = BTreeMap::new();

		while reader.position() < header_size as u64 {
			match signature.as_str() {
				"FArC" => {
					// Unencrypted, Compressed
					let name = reader.read_null_string()?;
					reader.seek(SeekFrom::Current(4))?;
					let compressed_length = reader.read_u32()?;
					let length = reader.read_u32()?;

					reader.seek(SeekFrom::Current(-12))?;
					let data = reader
						.read_pointer(move |reader| reader.read_buf(compressed_length as usize))?;
					reader.seek(SeekFrom::Current(8))?;

					if compressed_length == length {
						entries.insert(
							name,
							FarcEntry {
								data: BinaryParser::from_buf(data),
								modified_time: None,
							},
						);
						continue;
					}

					let mut cursor = Cursor::new(data);
					let mut decoder = gzip::Decoder::new(&mut cursor)?;
					let mut data = Vec::new();
					decoder.read_to_end(&mut data)?;
					entries.insert(
						name,
						FarcEntry {
							data: BinaryParser::from_buf(data),
							modified_time: Some(decoder.header().modification_time()),
						},
					);
				}
				"FArc" => {
					// Unencrypted, Uncompressed
					let name = reader.read_null_string()?;
					reader.seek(SeekFrom::Current(4))?;
					let length = reader.read_u32()?;

					reader.seek(SeekFrom::Current(-8))?;
					let data =
						reader.read_pointer(move |reader| reader.read_buf(length as usize))?;
					reader.seek(SeekFrom::Current(4))?;

					entries.insert(
						name,
						FarcEntry {
							data: BinaryParser::from_buf(data),
							modified_time: None,
						},
					);
				}
				_ => unimplemented!(),
			}
		}

		reader.set_big_endian(false);

		Ok(Self { entries, alignment })
	}

	pub fn write_file<P: AsRef<Path>>(self, path: P, compress: bool) -> Result<()> {
		let parser = self.write_parser(compress)?;
		let mut file = File::create(path)?;
		file.write(&parser.to_buf_const().unwrap())?;
		Ok(())
	}

	pub fn write_parser(self, compress: bool) -> Result<BinaryParser<'b>> {
		let mut entries = BTreeMap::new();
		for (name, entry) in self.entries {
			let data = entry.data.to_buf()?;
			entries.insert(name, (data, entry.modified_time));
		}

		let mut writer = BinaryParser::new();
		writer.set_big_endian(true);

		if compress {
			writer.write_string("FArC")?;
		} else {
			writer.write_string("FArc")?;
		}

		let entry_size = if compress { 12 } else { 8 };
		let header_size = entries
			.iter()
			.fold(4, |acc, (name, _)| acc + name.len() + 1 + entry_size);

		writer.write_u32(header_size as u32)?;
		writer.write_u32(self.alignment)?;

		for (name, (data, modified_time)) in entries {
			if compress {
				let length = data.len();

				let header = gzip::HeaderBuilder::new()
					.modification_time(modified_time.unwrap_or(0))
					.filename(std::ffi::CString::new(name.clone()).unwrap())
					.os(gzip::Os::Unix)
					.finish();
				let options = gzip::EncodeOptions::new().header(header);
				let mut encoder = gzip::Encoder::with_options(Vec::new(), options)?;

				encoder.write_all(&data)?;
				let data = encoder.finish().into_result()?;
				let compressed_length = data.len();

				writer.write_null_string(&name)?;
				writer.write_pointer(move |writer| {
					writer.write_buf(&data)?;
					writer.align_write_value(self.alignment as u64, 0x78)?;

					Ok(())
				})?;
				writer.write_u32(compressed_length as u32)?;
				writer.write_u32(length as u32)?;
			} else {
				let length = data.len();

				writer.write_null_string(&name)?;
				writer.write_pointer(move |writer| {
					writer.write_buf(&data)?;
					writer.align_write_value(self.alignment as u64, 0x78)?;

					Ok(())
				})?;
				writer.write_u32(length as u32)?;
			}
		}

		writer.align_write_value(self.alignment as u64, 0x78)?;
		let mut writer = writer.finish_writes()?;
		writer.align_write_value(self.alignment as u64, 0x78)?;

		Ok(writer)
	}
}
