// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_META_H_IS_INCLUDED
#define DMTR_META_H_IS_INCLUDED

#define DMTR_META_CONCAT2(A, B) A##B
#define DMTR_CONCAT(A, B) DMTR_META_CONCAT2(A, B)
#define DMTR_COUNTER __COUNTER__
#define DMTR_UNIQID(Prefix) DMTR_CONCAT(Prefix, DMTR_COUNTER)
#define DMTR_NOP() do { } while (0)

#endif /* DMTR_META_H_IS_INCLUDED */
