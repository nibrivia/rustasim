use slog::OwnedKVList;
use slog::Record;

use std::cell::RefCell;
use std::io;
use std::time::Instant;

/// Attempt to write a *very* simple logger
#[derive(Debug)]
pub struct MsgLogger<W: io::Write> {
    io: RefCell<W>,
    pub start: Instant,
}

impl<W> MsgLogger<W>
where
    W: io::Write,
{
    pub fn new(io: W) -> MsgLogger<W> {
        MsgLogger {
            io: RefCell::new(io),
            start: Instant::now(),
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
        if rinfo.level() == slog::Level::Trace {
            writeln!(io, "{},{}", self.start.elapsed().as_nanos(), rinfo.msg())?;
        } else {
            writeln!(io, "{}", rinfo.msg())?;
        }

        Ok(())
    }
}

// TODO serializer?
