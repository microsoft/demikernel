// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/raii_guard.hh>

#include <utility>

dmtr::raii_guard::raii_guard(raii_guard &&other) :
    my_dtor(std::move(other.my_dtor))
{
    other.cancel();
}

void dmtr::raii_guard::cancel() {
    my_dtor = []{};
}
