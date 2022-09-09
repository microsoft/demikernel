// Copyright(c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    perftools::profiler,
    timer,
};

#[test]
fn test_multiple_roots() {
    profiler::reset();

    for i in 0..=5 {
        if i == 5 {
            timer!("a");
        }
        {
            timer!("b");
        }
    }

    profiler::PROFILER.with(|p| {
        let p = p.borrow();

        assert_eq!(p.roots.len(), 2);

        for root in p.roots.iter() {
            assert!(root.borrow().pred.is_none());
            assert!(root.borrow().succs.is_empty());
        }

        assert_eq!(p.roots[0].borrow().name, "b");
        assert_eq!(p.roots[1].borrow().name, "a");

        assert_eq!(p.roots[0].borrow().num_calls, 6);
        assert_eq!(p.roots[1].borrow().num_calls, 1);
    });
}

#[test]
fn test_succ_reuse() {
    use std::ptr;

    profiler::reset();

    for i in 0..=5 {
        timer!("a");
        if i > 2 {
            timer!("b");
        }
    }

    assert_eq!(profiler::PROFILER.with(|p| p.borrow().roots.len()), 1);

    profiler::PROFILER.with(|p| {
        let p = p.borrow();

        assert_eq!(p.roots.len(), 1);

        let root = p.roots[0].borrow();
        assert_eq!(root.name, "a");
        assert!(root.pred.is_none());
        assert_eq!(root.succs.len(), 1);
        assert_eq!(root.num_calls, 6);

        let succ = root.succs[0].borrow();
        assert_eq!(succ.name, "b");
        assert!(ptr::eq(succ.pred.as_ref().unwrap().as_ref(), p.roots[0].as_ref()));
        assert!(succ.succs.is_empty());
        assert_eq!(succ.num_calls, 3);
    });
}

#[test]
fn test_reset_during_frame() {
    profiler::reset();

    for i in 0..=5 {
        timer!("a");
        timer!("b");
        {
            timer!("c");
            if i == 5 {
                profiler::reset();
            }

            assert!(profiler::PROFILER.with(|p| p.borrow().current.is_some()));

            timer!("d");
        }
    }

    profiler::PROFILER.with(|p| {
        let p = p.borrow();

        assert!(p.roots.is_empty());
        assert!(p.current.is_none());
    });
}
