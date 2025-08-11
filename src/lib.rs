use binary_parser::*;
use libflate::gzip;
use std::{
	collections::BTreeMap,
	fs::File,
	io::{self, Cursor, Read, SeekFrom, Write},
	path::Path,
};
use thiserror::Error;

#[cfg(feature = "python")]
pub mod py;

pub struct Farc<'a> {
	pub entries: BTreeMap<String, BinaryParser<'a>>,
}

#[derive(Error, Debug)]
pub enum FarcError {
	#[error("{0}")]
	BinaryParserError(#[from] BinaryParserError),
	#[error("File unsupported")]
	Unsupported,
	#[error("{0}")]
	IoError(#[from] io::Error),
	#[error("Pending Writes, please call finish_writes on any entries that have been modified")]
	PendingWrites,
}

pub type Result<T> = std::result::Result<T, FarcError>;

impl<'a> Farc<'a> {
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
		let mut reader = BinaryParser::from_file(path)?;
		Self::from_parser(&mut reader)
	}

	pub fn from_parser(reader: &mut BinaryParser<'a>) -> Result<Self> {
		reader.set_big_endian(true);
		let signature = reader.read_string(4)?;
		let header_size = reader.read_u32()? + 8;
		let entries = match signature.as_str() {
			"FARC" => unimplemented!(),
			"FArC" => {
				let mut entries = BTreeMap::new();
				_ = reader.read_u32()?;

				while reader.position() < header_size as u64 {
					let name = reader.read_null_string()?;
					_ = reader.read_u32()?;
					let compressed_len = reader.read_u32()?;
					let uncompressed_len = reader.read_u32()?;

					reader.seek(SeekFrom::Current(-12))?;
					let data = reader
						.read_pointer(move |reader| reader.read_buf(compressed_len as usize))?;
					reader.seek(SeekFrom::Current(8))?;

					let data = if compressed_len != uncompressed_len {
						let mut cursor = Cursor::new(data);
						let mut decoder = gzip::Decoder::new(&mut cursor)?;
						let mut data = Vec::new();
						decoder.read_to_end(&mut data)?;
						BinaryParser::from_buf(data)
					} else {
						BinaryParser::from_buf(data)
					};

					entries.insert(name, data);
				}

				entries
			}
			"FArc" => {
				let mut entries = BTreeMap::new();
				_ = reader.read_u32()?;

				while reader.position() < header_size as u64 {
					let name = reader.read_null_string()?;
					_ = reader.read_u32()?;
					let length = reader.read_u32()?;

					reader.seek(SeekFrom::Current(-8))?;
					let data =
						reader.read_pointer(move |reader| reader.read_parser(length as usize))?;
					reader.seek(SeekFrom::Current(4))?;

					entries.insert(name, data);
				}

				entries
			}
			_ => return Err(FarcError::Unsupported),
		};
		reader.set_big_endian(false);
		Ok(Self { entries })
	}

	pub fn write_file<P: AsRef<Path>>(&self, path: P, compress: bool) -> Result<()> {
		let parser = self.write_parser(compress)?;
		let mut file = File::create(path)?;
		file.write(&parser.to_buf_const().unwrap())?;
		Ok(())
	}

	pub fn write_parser(&self, compress: bool) -> Result<BinaryParser<'_>> {
		if self
			.entries
			.iter()
			.find(|(_, data)| data.pending_writes())
			.is_some()
		{
			return Err(FarcError::PendingWrites);
		}

		let mut writer = BinaryParser::new();
		writer.set_big_endian(true);
		if compress {
			writer.write_string("FArC")?;
		} else {
			writer.write_string("FArc")?;
		}
		let size = if compress {
			self.entries
				.iter()
				.map(|(name, _)| name.len() + 1 + 12)
				.fold(0, |acc, elem| acc + elem)
		} else {
			self.entries
				.iter()
				.map(|(name, _)| name.len() + 1 + 8)
				.fold(0, |acc, elem| acc + elem)
		};

		writer.write_u32(size as u32)?;
		writer.write_u32(0)?;
		for (name, data) in &self.entries {
			if compress {
				let buf = data.to_buf_const().unwrap();
				let mut encoder = gzip::Encoder::new(vec![])?;
				encoder.write(buf)?;
				let data = encoder.finish().into_result()?;
				let compressed_len = data.len() as u32;
				let uncomprssed_len = buf.len() as u32;

				writer.write_null_string(name)?;
				writer.write_pointer(move |writer| writer.write_buf(&data))?;
				writer.write_u32(compressed_len)?;
				writer.write_u32(uncomprssed_len)?;
			} else {
				let data = data.to_buf_const().unwrap().clone();
				let len = data.len() as u32;

				writer.write_null_string(name)?;
				writer.write_pointer(move |writer| writer.write_buf(&data))?;
				writer.write_u32(len)?;
			};
		}
		let writer = writer.finish_writes()?;

		Ok(writer)
	}
}
