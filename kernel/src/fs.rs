//! Minimal RAM filesystem (tmpfs) for STAR OS kernel.
//! Inode-based with max 256 inodes, 4KB per-file buffers.

use alloc::string::String;
use alloc::vec::Vec;

const MAX_INODES: usize = 256;
const BUF_SIZE: usize = 4096;
const MAX_DIR_ENTRIES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FileType {
    Regular,
    Directory,
    CharDev,
    BlockDev,
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub inode_id: usize,
}

#[derive(Clone)]
struct Inode {
    ftype: FileType,
    data: Vec<u8>,
    size: usize,
    entries: Vec<DirEntry>,
    in_use: bool,
}

impl Inode {
    fn new(ftype: FileType) -> Self {
        Self {
            ftype,
            data: if ftype == FileType::Regular { vec![0u8; BUF_SIZE] } else { Vec::new() },
            size: 0,
            entries: Vec::new(),
            in_use: true,
        }
    }
}

pub struct RamFs {
    inodes: Vec<Inode>,
}

#[derive(Debug, PartialEq)]
pub enum FsError {
    NoSpace,
    NotFound,
    NotDirectory,
    NotFile,
    NameTooLong,
    DirFull,
    InvalidInode,
}

pub type FsResult<T> = Result<T, FsError>;

impl RamFs {
    pub fn new() -> Self {
        let mut inodes = Vec::with_capacity(MAX_INODES);
        let root = Inode::new(FileType::Directory);
        inodes.push(root);
        for _ in 1..MAX_INODES {
            inodes.push(Inode { ftype: FileType::Regular, data: Vec::new(), size: 0, entries: Vec::new(), in_use: false });
        }
        Self { inodes }
    }

    fn alloc_inode(&mut self, ftype: FileType) -> FsResult<usize> {
        for i in 1..MAX_INODES {
            if !self.inodes[i].in_use {
                self.inodes[i] = Inode::new(ftype);
                return Ok(i);
            }
        }
        Err(FsError::NoSpace)
    }

    pub fn create(&mut self, parent: usize, name: &str, ftype: FileType) -> FsResult<usize> {
        if name.len() > 255 { return Err(FsError::NameTooLong); }
        if !self.inodes[parent].in_use || self.inodes[parent].ftype != FileType::Directory {
            return Err(FsError::NotDirectory);
        }
        if self.inodes[parent].entries.len() >= MAX_DIR_ENTRIES {
            return Err(FsError::DirFull);
        }
        let id = self.alloc_inode(ftype)?;
        self.inodes[parent].entries.push(DirEntry { name: String::from(name), inode_id: id });
        Ok(id)
    }

    pub fn mkdir(&mut self, parent: usize, name: &str) -> FsResult<usize> {
        self.create(parent, name, FileType::Directory)
    }

    pub fn lookup(&self, parent: usize, name: &str) -> FsResult<usize> {
        if parent >= MAX_INODES || !self.inodes[parent].in_use {
            return Err(FsError::InvalidInode);
        }
        if self.inodes[parent].ftype != FileType::Directory {
            return Err(FsError::NotDirectory);
        }
        for e in &self.inodes[parent].entries {
            if e.name == name { return Ok(e.inode_id); }
        }
        Err(FsError::NotFound)
    }

    pub fn open(&self, inode: usize) -> FsResult<FileType> {
        if inode >= MAX_INODES || !self.inodes[inode].in_use {
            return Err(FsError::InvalidInode);
        }
        Ok(self.inodes[inode].ftype)
    }

    pub fn read(&self, inode: usize, offset: usize, buf: &mut [u8]) -> FsResult<usize> {
        if inode >= MAX_INODES || !self.inodes[inode].in_use {
            return Err(FsError::InvalidInode);
        }
        if self.inodes[inode].ftype != FileType::Regular {
            return Err(FsError::NotFile);
        }
        let node = &self.inodes[inode];
        if offset >= node.size { return Ok(0); }
        let avail = node.size - offset;
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&node.data[offset..offset + n]);
        Ok(n)
    }

    pub fn write(&mut self, inode: usize, offset: usize, data: &[u8]) -> FsResult<usize> {
        if inode >= MAX_INODES || !self.inodes[inode].in_use {
            return Err(FsError::InvalidInode);
        }
        if self.inodes[inode].ftype != FileType::Regular {
            return Err(FsError::NotFile);
        }
        let node = &mut self.inodes[inode];
        let end = (offset + data.len()).min(BUF_SIZE);
        let n = end - offset;
        node.data[offset..end].copy_from_slice(&data[..n]);
        if end > node.size { node.size = end; }
        Ok(n)
    }

    pub fn unlink(&mut self, parent: usize, name: &str) -> FsResult<()> {
        if parent >= MAX_INODES || !self.inodes[parent].in_use {
            return Err(FsError::InvalidInode);
        }
        if self.inodes[parent].ftype != FileType::Directory {
            return Err(FsError::NotDirectory);
        }
        let pos = self.inodes[parent].entries.iter().position(|e| e.name == name)
            .ok_or(FsError::NotFound)?;
        let id = self.inodes[parent].entries[pos].inode_id;
        self.inodes[parent].entries.remove(pos);
        self.inodes[id].in_use = false;
        self.inodes[id].data = Vec::new();
        self.inodes[id].entries = Vec::new();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_lookup() {
        let mut fs = RamFs::new();
        let id = fs.create(0, "hello.txt", FileType::Regular).unwrap();
        assert_eq!(fs.lookup(0, "hello.txt"), Ok(id));
    }

    #[test]
    fn test_read_write() {
        let mut fs = RamFs::new();
        let id = fs.create(0, "f", FileType::Regular).unwrap();
        let data = b"STAR OS";
        fs.write(id, 0, data).unwrap();
        let mut buf = [0u8; 16];
        let n = fs.read(id, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], data);
    }

    #[test]
    fn test_mkdir_and_nested() {
        let mut fs = RamFs::new();
        let dir = fs.mkdir(0, "subdir").unwrap();
        let file = fs.create(dir, "nested.txt", FileType::Regular).unwrap();
        assert_eq!(fs.lookup(dir, "nested.txt"), Ok(file));
    }

    #[test]
    fn test_unlink() {
        let mut fs = RamFs::new();
        fs.create(0, "tmp", FileType::Regular).unwrap();
        assert!(fs.unlink(0, "tmp").is_ok());
        assert_eq!(fs.lookup(0, "tmp"), Err(FsError::NotFound));
    }

    #[test]
    fn test_device_node() {
        let mut fs = RamFs::new();
        let id = fs.create(0, "tty0", FileType::CharDev).unwrap();
        assert_eq!(fs.open(id), Ok(FileType::CharDev));
    }
}
