use tokio::{
    cell::UnsafeCell,
    sync::{
        Arc,
        atomic::{
            AtomicUsize,Ordering
        }
    }
}

struct RingBuffer {
    buffer: Box<[u8]>,
    start: AtomicUsize,
    end: AtomicUsize,
}

pub struct Writer(Arc<RingBuffer>);

pub struct Reader(Arc<RingBuffer>);

fn create(size:usize)->(Writer,Reader){
    let rb=RingBuffer {
        buffer: vec![0u8; size+1].into_boxed_slice(),
        start: 0,
        end: 1,
    };

}

unsafe impl Sync for RingBuffer{}

impl RingBuffer {

    fn read_exact(&self, buf: &mut [u8]) -> Option<usize> {
        let (mut start, end) = self.get_bound();
        if start <= end {
            //数据没有断开
            let read = (&mut buf[..]).write(&self.buffer[start..end]).unwrap();
            self.start.store(start + read, Ordering::Relaxed);
            Some(read)
        } else {
            //数据断开了
            let r1 = (&mut buf[..]).write(&self.buffer[start..]).unwrap();
            start += r1;
            let r2 = (&mut buf[r1..]).write(&self.buffer[..end]).unwrap();
            start += r2;

            self.start.store(start, Ordering::Relaxed);
            Some(r1 + r2)
        }
    }

    fn free_cap(&self) -> usize {
        let (start, end) = self.get_bound();
        if start < end {
            self.buffer.len() - (end - start)
        } else {
            start - end
        }
    }

    fn get_bound(&self)->(usize,usize){
        (
            self.start.load(Ordering::Relaxed),
            self.end.load(Ordering::Relaxed)
        )
    }
}

impl Writer{
    fn write_exact(&mut self, buf: &[u8]) -> usize {
        let (start, mut end) = self.0.get_bound();
        //安全：已经保证不会写到读区域
        //强制将不可变引用改成可变引用
        let buffer = unsafe {
            (&*self.buffer as *const [u8] as *mut [u8])
                .as_mut()
                .unwrap()
        };
        if start < end {
            //数据没分片，那么空区就要分片
            let size = (&mut buffer[end..]).write(buf).unwrap();
            if size < buf.len() {
                //数据还没塞完
                let len = (&mut buffer[..start]).write(&buf[size..]).unwrap();
                self.end.store(len, Ordering::Relaxed);
                size + len
            } else {
                //一个空区塞得下
                self.end.store(end + size, Ordering::Relaxed);
                size
            }
        } else {
            //数据分片，空区就无需分片
            let size = (&mut buffer[end..start]).write(buf).unwrap();
            self.end.store(end + size, Ordering::Relaxed);
            size
        }
    }

}
