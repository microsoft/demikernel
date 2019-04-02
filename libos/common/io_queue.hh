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
#include <memory>
#include <sys/socket.h>
#include <unordered_map>

extern dmtr_timer_t *write_timer;
extern dmtr_timer_t *read_timer;

namespace dmtr {

class io_queue
{
    // todo: fix case.
    public: enum category_id {
        MEMORY_Q,
        NETWORK_Q,
        FILE_Q,
    };

    protected: class task {
        public: typedef boost::coroutines2::coroutine<void> coroutine_type;
        public: typedef coroutine_type::push_type yield_type;
        public: typedef int (*completion_type)(yield_type &, task &t, io_queue &);
        private: class completion_adaptor {
            private: task * const my_task;
            private: int * const my_error;
            private: io_queue * const my_queue;
            private: completion_type my_completion;
            public: completion_adaptor(task &t, int &error, io_queue &q, completion_type completion);
            public: completion_adaptor(task &t, int &error, io_queue &q, completion_type completion, const dmtr_sgarray_t &input);

            public: void operator()(yield_type &y) {
                *my_error = my_completion(y, *my_task, *my_queue);
            }
        };

        private: dmtr_qresult_t my_qr;
        private: int my_error;
        private: dmtr_sgarray_t my_sga_arg;
        private: io_queue *my_queue_arg;
        private: coroutine_type::pull_type my_coroutine;

        private: task();
        private: static int new_object(std::unique_ptr<task> &task_out, io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode);
        public: static int new_object(std::unique_ptr<task> &task_out, io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, completion_type completion);
        public: static int new_object(std::unique_ptr<task> &task_out, io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, completion_type completion, const dmtr_sgarray_t &arg);
        public: static int new_object(std::unique_ptr<task> &task_out, io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, completion_type completion, io_queue *arg);
        public: static int initialize_result(dmtr_qresult_t &qr, int qd, dmtr_qtoken_t qt);
        private: static void coroutine_nop(yield_type &);

        public: int poll(dmtr_qresult_t &qr_out);
        public: void complete(const dmtr_sgarray_t &sga);
        public: void complete(int qd, const sockaddr_in &addr, socklen_t len);
        public: bool arg(const dmtr_sgarray_t *&arg_out) const;
        public: bool arg(io_queue *&arg_out) const;

        private: void start_coroutine(completion_type completion, io_queue &q);

        public: bool done() const {
            return !my_coroutine;
        }

        public: int qd() const {
            return my_qr.qr_qd;
        }

        public: dmtr_qtoken_t qt() const {
            return my_qr.qr_qt;
        }

        public: dmtr_opcode_t opcode() const {
            return my_qr.qr_opcode;
        }
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

    public: static int set_non_blocking(int fd);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion, const dmtr_sgarray_t &arg);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion, io_queue *arg);
    private: int get_task(task *&t, dmtr_qtoken_t qt);
    private: int drop_task(dmtr_qtoken_t qt);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED */
