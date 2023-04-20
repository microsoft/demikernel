// Copyright(c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    perftools::profiler,
    timer,
};
use ::anyhow::Result;

#[test]
fn test_multiple_roots() -> Result<()> {
    profiler::reset();

    for i in 0..=5 {
        if i == 5 {
            timer!("a");
        }
        {
            timer!("b");
        }
    }

    profiler::PROFILER.with(|p| -> Result<()> {
        let p = p.borrow();

        crate::ensure_eq!(p.roots.len(), 2);

        for root in p.roots.iter() {
            crate::ensure_eq!(root.borrow().pred.is_none(), true);
            crate::ensure_eq!(root.borrow().succs.is_empty(), true);
        }

        crate::ensure_eq!(p.roots[0].borrow().name, "b");
        crate::ensure_eq!(p.roots[1].borrow().name, "a");

        crate::ensure_eq!(p.roots[0].borrow().num_calls, 6);
        crate::ensure_eq!(p.roots[1].borrow().num_calls, 1);

        Ok(())
    })
}

#[test]
fn test_succ_reuse() -> Result<()> {
    use std::ptr;

    profiler::reset();

    for i in 0..=5 {
        timer!("a");
        if i > 2 {
            timer!("b");
        }
    }

    crate::ensure_eq!(profiler::PROFILER.with(|p| p.borrow().roots.len()), 1);

    profiler::PROFILER.with(|p| -> Result<()> {
        let p = p.borrow();

        crate::ensure_eq!(p.roots.len(), 1);

        let root = p.roots[0].borrow();
        crate::ensure_eq!(root.name, "a");
        crate::ensure_eq!(root.pred.is_none(), true);
        crate::ensure_eq!(root.succs.len(), 1);
        crate::ensure_eq!(root.num_calls, 6);

        let succ = root.succs[0].borrow();
        crate::ensure_eq!(succ.name, "b");
        crate::ensure_eq!(ptr::eq(succ.pred.as_ref().unwrap().as_ref(), p.roots[0].as_ref()), true);
        crate::ensure_eq!(succ.succs.is_empty(), true);
        crate::ensure_eq!(succ.num_calls, 3);

        Ok(())
    })
}

#[test]
fn test_reset_during_frame() -> Result<()> {
    profiler::reset();

    for i in 0..=5 {
        timer!("a");
        timer!("b");
        {
            timer!("c");
            if i == 5 {
                profiler::reset();
            }

            crate::ensure_eq!(profiler::PROFILER.with(|p| p.borrow().current.is_some()), true);

            timer!("d");
        }
    }

    profiler::PROFILER.with(|p| -> Result<()> {
        let p = p.borrow();

        crate::ensure_eq!(p.roots.is_empty(), true);
        crate::ensure_eq!(p.current.is_none(), true);

        Ok(())
    })
}
