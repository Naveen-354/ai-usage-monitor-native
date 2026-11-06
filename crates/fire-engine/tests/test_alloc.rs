//! The animations are meant to be light: once running they must not touch the heap at all.
//! This test binary counts every allocation made by the thread that runs it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use fire_engine::engine::FlameState;
use fire_engine::{create, Quality, Style, XorShift64};

thread_local! {
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

fn bump() {
    let _ = COUNT.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        bump();
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn allocations() -> usize {
    COUNT.with(|c| c.get())
}

#[test]
fn running_and_painting_a_scene_never_allocates() {
    for style in Style::ALL.into_iter().filter(|s| *s != Style::Fire) {
        for q in Quality::ALL {
            let mut rng = XorShift64::new(5);
            let mut anim = create(style, q);
            let (w, h) = anim.dims();
            let mut buf = vec![0u8; w * h * 4];
            anim.set_state(FlameState::Live, 1.0, &mut rng);
            for _ in 0..10 {
                anim.tick(83.0, &mut rng);
                anim.paint(&mut buf);
            }
            let before = allocations();
            let mut data = fire_engine::scene::Data::default();
            for i in 0..120 {
                data.level = (i % 20) as f32 / 20.0;
                data.n_agents = (i % 8 + 1) as u8;
                anim.set_data(&data);
                if i % 20 == 0 {
                    anim.blast(10f64.powi(3 + (i / 20) % 5), &mut rng);
                }
                anim.tick(50.0, &mut rng);
                anim.paint(&mut buf);
            }
            assert_eq!(allocations() - before, 0, "{style:?} {q:?} allocated while running");
        }
    }
}
