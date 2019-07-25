// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_RAII_GUARD_HH_IS_INCLUDED
#define DMTR_RAII_GUARD_HH_IS_INCLUDED

#include <functional>
#include <utility>

namespace dmtr {

class raii_guard {
    private: std::function<void()> my_dtor;

    public: ~raii_guard() {
        my_dtor();
    }

    public:
        template <class Fun>
        raii_guard(Fun const &dtor) :
            my_dtor(dtor)
        {}

    private: raii_guard(const raii_guard &) = delete;
    public: raii_guard(raii_guard &&other);

    public: void cancel();
};

} //namespace dmtr

#endif /* DMTR_RAII_GUARD_HH_IS_INCLUDED */
