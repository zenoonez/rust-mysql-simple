use std::io::{IoResult, SeekCur, Seek, Reader, Writer};
use super::value::{Value, NULL, Int, UInt, Float, Bytes, Date, Time};
use super::consts;
use std::io::net::{tcp, pipe};
#[cfg(feature = "openssl")]
use openssl::ssl;

pub trait MyReader: Reader + Seek {
	fn read_lenenc_int(&mut self) -> IoResult<u64> {
		let head_byte = try!(self.read_u8());
		let mut length;
		match head_byte {
			0xfc => length = 2,
			0xfd => length = 3,
			0xfe => length = 8,
			x => return Ok(x as u64)
		}
		return self.read_le_uint_n(length);
	}

    fn skip_lenenc_int(&mut self) -> IoResult<()> {
        match try!(self.read_u8()) {
            0xfc => self.seek(2, SeekCur),
            0xfd => self.seek(3, SeekCur),
            0xfe => self.seek(8, SeekCur),
            _ => Ok(())
        }
    }

	fn read_lenenc_bytes(&mut self) -> IoResult<Vec<u8>> {
		let len = try!(self.read_lenenc_int());
		if len > 0 {
			self.read_exact(len as uint)
		} else {
			Ok(Vec::with_capacity(0))
		}
	}

    fn skip_lenenc_bytes(&mut self) -> IoResult<()> {
        let len = try!(self.read_lenenc_int());
        if len > 0 {
            self.seek(len as i64, SeekCur)
        } else {
            Ok(())
        }
    }

	fn read_to_null(&mut self) -> IoResult<Vec<u8>> {
		let mut buf = Vec::new();
		let mut x = try!(self.read_u8());
		while x != 0u8 {
			buf.push(x);
			x = try!(self.read_u8());
		}
		Ok(buf)
	}

	fn read_bin_value(&mut self, column_type: consts::ColumnType, unsigned: bool) -> IoResult<Value> {
		match column_type {
            consts::MYSQL_TYPE_STRING |
            consts::MYSQL_TYPE_VAR_STRING |
            consts::MYSQL_TYPE_BLOB |
            consts::MYSQL_TYPE_TINY_BLOB |
            consts::MYSQL_TYPE_MEDIUM_BLOB |
            consts::MYSQL_TYPE_LONG_BLOB |
            consts::MYSQL_TYPE_SET |
            consts::MYSQL_TYPE_ENUM |
            consts::MYSQL_TYPE_DECIMAL |
            consts::MYSQL_TYPE_VARCHAR |
            consts::MYSQL_TYPE_BIT |
            consts::MYSQL_TYPE_NEWDECIMAL |
            consts::MYSQL_TYPE_GEOMETRY => {
                Ok(Bytes(try!(self.read_lenenc_bytes())))
            },
            consts::MYSQL_TYPE_TINY => {
                if unsigned {
                    Ok(Int(try!(self.read_u8()) as i64))
                } else {
                    Ok(Int(try!(self.read_i8()) as i64))
                }
            },
            consts::MYSQL_TYPE_SHORT |
            consts::MYSQL_TYPE_YEAR => {
                if unsigned {
                    Ok(Int(try!(self.read_le_u16()) as i64))
                } else {
                    Ok(Int(try!(self.read_le_i16()) as i64))
                }
            },
            consts::MYSQL_TYPE_LONG |
            consts::MYSQL_TYPE_INT24 => {
                if unsigned {
                    Ok(Int(try!(self.read_le_u32()) as i64))
                } else {
                    Ok(Int(try!(self.read_le_i32()) as i64))
                }
            },
            consts::MYSQL_TYPE_LONGLONG => {
                if unsigned {
                    Ok(UInt(try!(self.read_le_u64())))
                } else {
                    Ok(Int(try!(self.read_le_i64()) as i64))
                }
            },
            consts::MYSQL_TYPE_FLOAT => {
                Ok(Float(try!(self.read_le_f32()) as f64))
            },
            consts::MYSQL_TYPE_DOUBLE => {
                Ok(Float(try!(self.read_le_f64())))
            },
            consts::MYSQL_TYPE_TIMESTAMP |
            consts::MYSQL_TYPE_DATE |
            consts::MYSQL_TYPE_DATETIME => {
                let len = try!(self.read_u8());
                let mut year = 0u16;
                let mut month = 0u8;
                let mut day = 0u8;
                let mut hour = 0u8;
                let mut minute = 0u8;
                let mut second = 0u8;
                let mut micro_second = 0u32;
                if len >= 4u8 {
                    year = try!(self.read_le_u16());
                    month = try!(self.read_u8());
                    day = try!(self.read_u8());
                }
                if len >= 7u8 {
                    hour = try!(self.read_u8());
                    minute = try!(self.read_u8());
                    second = try!(self.read_u8());
                }
                if len == 11u8 {
                    micro_second = try!(self.read_le_u32());
                }
                Ok(Date(year, month, day, hour, minute, second, micro_second))
            },
            consts::MYSQL_TYPE_TIME => {
                let len = try!(self.read_u8());
                let mut is_negative = false;
                let mut days = 0u32;
                let mut hours = 0u8;
                let mut minutes = 0u8;
                let mut seconds = 0u8;
                let mut micro_seconds = 0u32;
                if len >= 8u8 {
                    is_negative = try!(self.read_u8()) == 1u8;
                    days = try!(self.read_le_u32());
                    hours = try!(self.read_u8());
                    minutes = try!(self.read_u8());
                    seconds = try!(self.read_u8());
                }
                if len == 12u8 {
                    micro_seconds = try!(self.read_le_u32());
                }
                Ok(Time(is_negative, days, hours, minutes, seconds, micro_seconds))
            }
            _ => Ok(NULL)
        }
	}
}

impl<T:Reader + Seek> MyReader for T {}

pub trait MyWriter: Writer {
	fn write_le_uint_n(&mut self, x: u64, len: uint) -> IoResult<()> {
		let mut buf = Vec::from_elem(len, 0u8);
		let mut offset = 0;
		while offset < len {
			*buf.get_mut(offset) = (((0xff << (offset * 8)) & x) >> (offset * 8)) as u8;
			offset += 1;
		}
		self.write(buf.as_slice())
	}

	fn write_lenenc_int(&mut self, x: u64) -> IoResult<()> {
		if x < 251 {
			self.write_le_uint_n(x, 1)
		} else if x < 65_536 {
			try!(self.write_u8(0xfc));
			self.write_le_uint_n(x, 2)
		} else if x < 16_777_216 {
			try!(self.write_u8(0xfd));
            self.write_le_uint_n(x, 3)
		} else {
			try!(self.write_u8(0xfe));
            self.write_le_uint_n(x, 8)
		}
	}

	fn write_lenenc_bytes(&mut self, bytes: &[u8]) -> IoResult<()> {
		try!(self.write_lenenc_int(bytes.len() as u64));
		self.write(bytes)
	}
}

impl<T:Writer> MyWriter for T {}

pub enum MyStream {
    #[cfg(feature = "openssl")]
    SecureStream(ssl::SslStream<PlainStream>),
    InsecureStream(PlainStream),
}

#[cfg(feature = "ssl")]
impl Reader for MyStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<uint> {
        match *self {
            SecureStream(ref mut s) => s.read(buf),
            InsecureStream(ref mut s) => s.read(buf),
        }
    }
}

#[cfg(not(feature = "ssl"))]
impl Reader for MyStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<uint> {
        match *self {
            InsecureStream(ref mut s) => s.read(buf),
        }
    }
}

#[cfg(feature = "ssl")]
impl Writer for MyStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        match *self {
            SecureStream(ref mut s) => s.write(buf),
            InsecureStream(ref mut s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        match *self {
            SecureStream(ref mut s) => s.flush(),
            InsecureStream(ref mut s) => s.flush(),
        }
    }
}

#[cfg(not(feature = "ssl"))]
impl Writer for MyStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        match *self {
            InsecureStream(ref mut s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        match *self {
            InsecureStream(ref mut s) => s.flush(),
        }
    }
}

pub enum PlainStream {
    TCPStream(tcp::TcpStream),
    UNIXStream(pipe::UnixStream),
}

impl Reader for PlainStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<uint> {
        match *self {
            TCPStream(ref mut s) => s.read(buf),
            UNIXStream(ref mut s) => s.read(buf),
        }
    }
}

impl Writer for PlainStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        match *self {
            TCPStream(ref mut s) => s.write(buf),
            UNIXStream(ref mut s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        match *self {
            TCPStream(ref mut s) => s.flush(),
            UNIXStream(ref mut s) => s.flush(),
        }
    }
}
