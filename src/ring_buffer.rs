use std::{
    cell::UnsafeCell,
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Weak,
    },
};

//start前要保持有一个空位，用于区别优弧和劣弧
//这意味着缓冲区的空位是end -> start-1

struct RingBuffer {
    buffer: UnsafeCell<Box<[u8]>>,
    capacity: usize,
    start: AtomicUsize,
    end: AtomicUsize,
}

pub struct Writer(Weak<RingBuffer>);

pub struct Reader(Arc<RingBuffer>);

pub fn create(size: usize) -> (Writer, Reader) {
    assert_ne!(size, usize::MAX);
    let rb = Arc::new(RingBuffer {
        buffer: UnsafeCell::new(vec![0u8; size + 1].into_boxed_slice()),
        capacity: size, //只读
        start: AtomicUsize::new(0),
        end: AtomicUsize::new(0),
    });
    (Writer(Arc::downgrade(&rb)), Reader(rb))
}

unsafe impl Sync for RingBuffer {}

impl RingBuffer {
    fn free_cap(&self) -> usize {
        let (start, end) = self.get_bound();
        if start < end {
            self.capacity - (end - start) - 1
        } else {
            start - end - 1
        }
    }

    fn get_bound(&self) -> (usize, usize) {
        (
            self.start.load(Ordering::Relaxed),
            self.end.load(Ordering::Relaxed),
        )
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let arc = match self.0.upgrade() {
            Some(arc) => arc,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "The reader part has been dropped",
                ));
            }
        };

        let (start, end) = arc.get_bound();
        //安全：已经保证不会写到读区域
        //强制将不可变引用改成可变引用
        let buffer = unsafe { arc.buffer.get().as_mut().unwrap() };

        let (end_index, size) = if start <= end {
            //数据没分片，那么空区就要分片
            let size = (&mut buffer[end..]).write(buf).unwrap();
            if size < buf.len() {
                //数据还没塞完
                let len = (&mut buffer[..start - 1]).write(&buf[size..]).unwrap();
                (len, size + len)
            } else {
                //一个空区塞得下
                (end + size, size)
            }
        } else {
            //数据分片，空区就无需分片
            let size = (&mut buffer[end..start - 1]).write(buf).unwrap();
            (end + size, size)
        };

        arc.end.store(end_index, Ordering::Relaxed);
        Ok(size)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (mut start, end) = self.0.get_bound();
        let buffer = unsafe { self.0.buffer.get().as_ref().unwrap() };

        if start <= end {
            //数据没有断开
            let read = (&mut buf[..]).write(&buffer[start..end]).unwrap();
            self.0.start.store(start + read, Ordering::Relaxed);
            Ok(read)
        } else {
            //数据断开了
            let r1 = (&mut buf[..]).write(&buffer[start..]).unwrap();
            start += r1;
            let r2 = (&mut buf[r1..]).write(&buffer[..end]).unwrap();
            start += r2;

            self.0.start.store(start, Ordering::Relaxed);
            Ok(r1 + r2)
        }
    }
}
