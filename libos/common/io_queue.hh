// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/queue.h
 *   Basic queue
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED

#include <boost/coroutine2/coroutine.hpp>
#include <dmtr/annot.h>
#include <dmtr/types.h>
#include <functional>
#include <memory>
#include <sys/socket.h>
#include <unordered_map>

namespace dmtr {

class io_queue
{
    // todo: fix case.
    public: enum category_id {
        MEMORY_Q,
        NETWORK_Q,
        FILE_Q,
    };


    // todo: reorder largest to smallest.
    protected: class task {
        public: typedef boost::coroutines2::coroutine<void> coroutine_type;
        public: typedef coroutine_type::push_type yield_type;
        public: typedef std::function<int (yield_type &, dmtr_qresult_t &qr_out)> completion_type;

        private: dmtr_qresult_t my_qr;
        private: int my_error;
        private: coroutine_type::pull_type my_coroutine;

        private: task();
        public: static int new_object(std::unique_ptr<task> &task_out, completion_type completion);
        public: int poll(dmtr_qresult_t &qr_out);
    };

    private: std::unordered_map<dmtr_qtoken_t, std::unique_ptr<task>> my_tasks;
    protected: const category_id my_cid;
    protected: const int my_qd;

    protected: io_queue(enum category_id cid, int qd);
    public: virtual ~io_queue();

    public: int qd() const {
        return my_qd;
    };

    public: enum category_id cid() {
        return my_cid;
    }

    // network control plane functions
    // todo: move into derived class.
    public: virtual int socket(int domain, int type, int protocol);
    public: virtual int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: virtual int listen(int backlog);
    public: virtual int bind(const struct sockaddr * const saddr, socklen_t size);
    public: virtual int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int new_qd);
    public: virtual int connect(const struct sockaddr * const saddr, socklen_t size);

    // general control plane functions.
    public: virtual int close();

    // data plane functions
    public: virtual int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) = 0;
    public: virtual int pop(dmtr_qtoken_t qt) = 0;
    public: virtual int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);
    public: virtual int drop(dmtr_qtoken_t qt);

    protected: static int set_non_blocking(int fd);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion);
    private: int get_task(task *&t, dmtr_qtoken_t qt);
    private: int drop_task(dmtr_qtoken_t qt);
    protected: void set_qresult(dmtr_qresult_t &qr_out, const dmtr_sgarray_t &sga) const;
    protected: void set_qresult(dmtr_qresult_t &qr_out, int qd, const sockaddr_in &addr, socklen_t len) const;
    protected: int init_qresult(dmtr_qresult_t &qr, dmtr_qtoken_t qt, int qd) const;
    protected: int init_qresult(dmtr_qresult_t &qr, dmtr_qtoken_t qt) const;
};

} // namespace dmtr

#endif /* DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED */
