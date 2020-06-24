use slog;
use slog::OwnedKVList;
use slog::Record;

use std::cell::RefCell;
use std::io;

/// Attempt to write a *very* simple logger
pub struct MsgLogger<W: io::Write> {
    io: RefCell<W>,
}

impl<W> MsgLogger<W>
where
    W: io::Write,
{
    pub fn new(io: W) -> MsgLogger<W> {
        MsgLogger {
            io: RefCell::new(io),
        }
    }
}

impl<W> slog::Drain for MsgLogger<W>
where
    W: io::Write,
{
    type Ok = ();
    type Err = io::Error;

    fn log(&self, rinfo: &Record, _logger_values: &OwnedKVList) -> io::Result<()> {
        let mut io = self.io.borrow_mut();
        write!(io, "{}\n", rinfo.msg())?;
        Ok(())
    }
}

// TODO serializer?

