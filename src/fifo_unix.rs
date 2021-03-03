use super::{Result, Error, OnCleanup};
use std::path::{Path, PathBuf};
use rand::{thread_rng, Rng, distributions::Alphanumeric};
use fcntl::OFlag;
use nix::{fcntl, unistd};
use nix::sys::stat::{stat, Mode, SFlag};
use nix::errno::Errno;
use nix::sys::termios::{tcflush, FlushArg};

/// Abstraction over a named pipe
pub struct Fifo
{
    handle: std::os::unix::io::RawFd,
    pub(super) path: PathBuf,
    pub(super) is_closed: bool,
    delete: OnCleanup
}

impl Fifo
{
    /// Open an existing pipe. If 'delete_on_drop' is true, the named pipe will
    /// be deleted when the returned struct is deallocated.
    pub fn open(path: &Path, delete_on_drop: OnCleanup) -> Result<Self>
    {
        let mode = Mode::S_IWUSR | Mode::S_IRUSR 
                 | Mode::S_IRGRP | Mode::S_IWGRP;

        if let Some(_) = path.parent()
        {
            match stat(path)
            {
                Ok(file_stat) => 
                {
                    // Error out if file is not a named pipe
                    if file_stat.st_mode & SFlag::S_IFIFO.bits() == 0
                    {
                        Err(nix::Error::InvalidPath)?;
                    }
                },
                err => 
                {
                    err?;
                }
            }

            fcntl::open(path, OFlag::O_RDWR | OFlag::O_NOCTTY, mode)
                .map(|handle| Fifo 
                    { 
                        handle, 
                        path: path.to_path_buf(), 
                        is_closed: false,
                        delete: delete_on_drop
                    }).map_err(Error::from)
        }
        else
        {
            Err(Error::InvalidPath)
        }
    }

    /// Create a pipe. If 'delete_on_drop' is true, the named pipe will be
    /// deleted when the returned struct is deallocated.
    pub fn create(delete_on_drop: OnCleanup) -> Result<Self>
    {
        let mode = Mode::S_IWUSR | Mode::S_IRUSR 
                 | Mode::S_IRGRP | Mode::S_IWGRP;

        // Generate a random path name
        let path = PathBuf::from(format!("/tmp/pipe_{}_{}", std::process::id(), thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .collect::<String>()));

        if let Some(_) = path.parent()
        {
            match stat(&path)
            {
                Ok(file_stat) => 
                {
                    // Error out if file is not a named pipe
                    if file_stat.st_mode & SFlag::S_IFIFO.bits() == 0
                    {
                        Err(Error::InvalidPath)?;
                    }
                },
                Err(nix::Error::InvalidPath) | Err(nix::Error::Sys(Errno::ENOENT)) => 
                {
                    unistd::mkfifo(&path, mode)?;
                },
                err => 
                {
                    err?;
                }
            }

            Fifo::open(&path, delete_on_drop)
        }
        else
        {
            Err(Error::InvalidPath)
        }
    }

    /// Close the pipe. If the pipe is not closed before deallocation, this will
    /// be called automatically on drop.
    pub fn close(&mut self) -> Result<()>
    {
        self.is_closed = true;
        unistd::close(self.handle).map_err(Error::from)
    }

    /// Write a single byte to the pipe
    pub fn write_byte(&mut self, buf: u8) -> Result<usize>
    {
        unistd::write(self.handle, &[buf]).map_err(Error::from)
    }

    /// Write an array of bytes to the pipe
    pub fn write_bytes(&mut self, buf: &[u8]) -> Result<usize>
    {
        unistd::write(self.handle, buf).map_err(Error::from)
    }

    /// Writes a string to the pipe
    pub fn write_string(&mut self, s: &str) -> Result<usize>
    {
        unistd::write(self.handle, s.as_bytes()).map_err(Error::from)
    }

    /// Read a single byte
    pub fn read_byte(&mut self) -> Result<u8>
    {
        let buf: &mut [u8; 1] = &mut [0];
        unistd::read(self.handle, buf)?;
        Ok(buf[0])
    }

    /// Reads the given number of bytes and returns the result in a vector.
    pub fn read_bytes(&mut self, size: usize) -> Result<Vec<u8>>
    {
        let mut buf: Vec<u8> = Vec::with_capacity(size);
        unistd::read(self.handle, &mut buf)?;
        Ok(buf)
    }

    /// Reads the given number of bytes and returns the result as a string.
    pub fn read_string(&mut self, size: usize) -> Result<String>
    {
        let mut buf: Vec<u8> = Vec::with_capacity(size);
        unistd::read(self.handle, &mut buf)?;
        String::from_utf8(buf).map_err(Error::from)
    }

    /// Flush input and output.
    pub fn flush_pipe(&self) -> Result<()>
    {
        tcflush(self.handle, FlushArg::TCIOFLUSH).map_err(Error::from)
    }
}

impl std::io::Read for Fifo 
{
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> 
    {
        unistd::read(self.handle, bytes)
            .map_err(Error::from)
            .map_err(std::io::Error::from)
    }
}

impl Drop for Fifo
{
    fn drop(&mut self) 
    {
        if !self.is_closed
        {
            if let Err(e) = self.close()
            {
                eprintln!("Error closing pipe: {:?}", e)
            }
        }

        if let OnCleanup::Delete = self.delete
        {
            std::fs::remove_file(&self.path).expect("Pipe removal Failed")
        }
    }
}

impl Clone for Fifo
{
    /// Cloning a fifo creates a slave which points to the same path but does not
    /// close the fifo when dropped.
    fn clone(&self) -> Self 
    {
        Fifo 
        { 
            handle: self.handle,
            path: self.path.clone(), 
            is_closed: true,
            delete: OnCleanup::NoDelete
        }
    }
}