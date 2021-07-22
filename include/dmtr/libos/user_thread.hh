// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_USER_THREAD_HH_IS_INCLUDED
#define DMTR_LIBOS_USER_THREAD_HH_IS_INCLUDED

#include <boost/coroutine2/coroutine.hpp>
#include <functional>
#include <queue>

namespace dmtr {

template <typename Value>
class user_thread {
    public: typedef std::queue<Value> queue_type;
    public: typedef boost::coroutines2::coroutine<void> coroutine_type;
    public: typedef coroutine_type::push_type yield_type;
    public: typedef std::function<int (yield_type &, queue_type &)> function_type;

    private: int my_error;
    private: std::unique_ptr<queue_type> my_queue;
    private: std::unique_ptr<coroutine_type::pull_type> my_coroutine;

    public: user_thread(function_type fun) :
        my_error(EAGAIN),
        my_queue(new queue_type)
    {
        my_coroutine.reset(new coroutine_type::pull_type([=](yield_type &yield) {
            my_error = fun(yield, *my_queue);
            if (EAGAIN == my_error) {
                DMTR_PANIC("User thread function may not return `EAGAIN`.");
            }
        }));
    }

    private: user_thread(const user_thread &) = delete;

    public: user_thread(user_thread &&other) :
        my_error(other.error),
        my_coroutine(std::move(other.my_coroutine)),
        my_queue(std::move(other.my_queue))
    {
        other.my_error = EAGAIN;
    }

    public: bool done() const {
        return !(bool)*my_coroutine;
    }

    public: void enqueue(const Value &value) {
        my_queue->push(value);
    }

    public: int service() {
        if (!done()) {
            (*my_coroutine)();
        }

        if (!done()) {
            return EAGAIN;
        }

        DMTR_TRUE(ENOTSUP, my_error != EAGAIN);
        return my_error;
    }

};

} //namespace dmtr

#endif /* DMTR_LIBOS_USER_THREAD_HH_IS_INCLUDED */

