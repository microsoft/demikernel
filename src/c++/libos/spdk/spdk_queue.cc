// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "spdk_queue.hh"

#include <boost/chrono.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/sga.h>
#include <iostream>
#include <dmtr/libos/mem.h>
#include <dmtr/libos/raii_guard.hh>
#include <spdk/env.h>
#include <spdk/log.h>
#include <spdk/nvme.h>
#include <unistd.h>

namespace bpo = boost::program_options;

namespace {
    static constexpr char kTrTypeString[] = "trtype=";
    static constexpr char kTrAddrString[] = "traddr=";
}

#if DMTR_PROFILE
typedef std::unique_ptr<dmtr_latency_t, std::function<void(dmtr_latency_t *)>> latency_ptr_type;
static latency_ptr_type read_latency;
static latency_ptr_type write_latency;
#endif

bool dmtr::spdk_queue::our_spdk_init_flag = false;

// Spdk static information.
struct spdk_nvme_ns *dmtr::spdk_queue::ns = nullptr;
struct spdk_nvme_qpair *dmtr::spdk_queue::qpair = nullptr;
int dmtr::spdk_queue::namespaceId = 1;
unsigned int dmtr::spdk_queue::namespaceSize = 0;
unsigned int dmtr::spdk_queue::sectorSize = 0;

char *dmtr::spdk_queue::partialBlock = nullptr;

dmtr::spdk_queue::spdk_queue(int qd, dmtr::io_queue::category_id cid) :
    lwip_queue(qd)
{}

//*****************************************************************************
// SPDK functions

bool probeCb(void *cb_ctx, const struct spdk_nvme_transport_id *trid, struct spdk_nvme_ctrlr_opts *opts) {
  // Always say that we would like to attach to the controller since we aren't
  // really looking for anything specific.
  return true;
}

void attachCb(void *cb_ctx, const struct spdk_nvme_transport_id *trid, struct spdk_nvme_ctrlr *cntrlr, const struct spdk_nvme_ctrlr_opts *opts) {
  struct spdk_nvme_io_qpair_opts qpopts;

  if (dmtr::spdk_queue::qpair != nullptr) {
    SPDK_ERRLOG("Already attached to a qpair\n");
    return;
  }

  dmtr::spdk_queue::ns = spdk_nvme_ctrlr_get_ns(cntrlr, dmtr::spdk_queue::namespaceId);

  if (dmtr::spdk_queue::ns == nullptr) {
    SPDK_ERRLOG("Can't get namespace by id %d\n", dmtr::spdk_queue::namespaceId);
    return;
  }

  if (!spdk_nvme_ns_is_active(dmtr::spdk_queue::ns)) {
    SPDK_ERRLOG("Inactive namespace at id %d\n", dmtr::spdk_queue::namespaceId);
    return;
  }

  spdk_nvme_ctrlr_get_default_io_qpair_opts(cntrlr, &qpopts, sizeof(qpopts));
  // TODO(ashmrtnz): If we want to change queue options like delaying the
  // doorbell, changing the queue size, or anything like that, we need to do it
  // here.
  
  dmtr::spdk_queue::qpair = spdk_nvme_ctrlr_alloc_io_qpair(cntrlr, &qpopts, sizeof(qpopts));
  if (!dmtr::spdk_queue::qpair) {
    SPDK_ERRLOG("Unable to allocate nvme qpair\n");
    return;
  }
  dmtr::spdk_queue::namespaceSize = spdk_nvme_ns_get_size(dmtr::spdk_queue::ns);
  if (dmtr::spdk_queue::namespaceSize <= 0) {
    SPDK_ERRLOG("Unable to get namespace size for namespace %d\n",
        dmtr::spdk_queue::namespaceId);
    return;
  }
  dmtr::spdk_queue::sectorSize = spdk_nvme_ns_get_sector_size(dmtr::spdk_queue::ns);
  // Allocate a buffer for writes that fill a partial block so that we don't
  // have to do a read-copy-update in the write path.
  dmtr::spdk_queue::partialBlock = (char *) malloc(dmtr::spdk_queue::sectorSize);
  if (dmtr::spdk_queue::partialBlock == nullptr) {
      SPDK_ERRLOG("Unable to allocate the partial block of size %d\n",
          dmtr::spdk_queue::sectorSize);
      return;
  }
}

// Right now only works for PCIe-based NVMe drives where the user specifies the
// address of a single device.
int dmtr::spdk_queue::parseTransportId(
    struct spdk_nvme_transport_id *trid, std::string &transportType,
    std::string &devAddress) {
  struct spdk_pci_addr pci_addr;
  std::string trinfo = std::string(kTrTypeString) + transportType + " " + kTrAddrString +
      devAddress;
  memset(trid, 0, sizeof(*trid));
  trid->trtype = SPDK_NVME_TRANSPORT_PCIE;
  if (spdk_nvme_transport_id_parse(trid, trinfo.c_str()) < 0) {
    SPDK_ERRLOG("Failed to parse transport type and device %s\n",
        trinfo.c_str());
    return -1;
  }
  if (trid->trtype != SPDK_NVME_TRANSPORT_PCIE) {
    SPDK_ERRLOG("Unsupported transport type and device %s\n",
        trinfo.c_str());
    return -1;
  }
  if (spdk_pci_addr_parse(&pci_addr, trid->traddr) < 0) {
    SPDK_ERRLOG("invalid device address %s\n", devAddress.c_str());
    return -1;
  }
  spdk_pci_addr_fmt(trid->traddr, sizeof(trid->traddr), &pci_addr);
  return 0;
}

 
int dmtr::spdk_queue::init_spdk(int argc, char *argv[]) {
    DMTR_TRUE(ERANGE, argc >= 0);
    if (argc > 0) {
        DMTR_NOTNULL(EINVAL, argv);
    }
    DMTR_TRUE(EPERM, !our_init_flag);

    std::string config_path;
    bpo::options_description desc("Allowed options");
    desc.add_options()
        ("help", "display usage information")
        ("config-path,c", bpo::value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file");

    bpo::variables_map vm;
    bpo::store(bpo::command_line_parser(argc, argv).options(desc).allow_unregistered().run(), vm);
    bpo::notify(vm);

    if (vm.count("help")) {
        std::cout << desc << std::endl;
        return 0;
    }

    if (access(config_path.c_str(), R_OK) == -1) {
        std::cerr << "Unable to find config file at `" << config_path << "`." << std::endl;
        return ENOENT;
    }
    YAML::Node config = YAML::LoadFile(config_path);

    if (our_spdk_init_flag) {
        return 0;
    }

    std::string transportType;
    std::string devAddress;
    // Initialize spdk from YAML options.
    YAML::Node node = config["spdk"]["transport"];
    if (YAML::NodeType::Scalar == node.Type()) {
        transportType = node.as<std::string>();
    }
    node = config["spdk"]["devAddr"];
    if (YAML::NodeType::Scalar == node.Type()) {
        devAddress = node.as<std::string>();
    }
    node = config["spdk"]["namespaceId"];
    if (YAML::NodeType::Scalar == node.Type()) {
        namespaceId = node.as<unsigned int>();
    }

    struct spdk_env_opts opts;
    return init_spdk(spdk_env_opts &opts);
}

int dmtr::spdk_queue::init_spdk(spdk_env_opts *opts)
{
    spdk_env_opts_init(opts);
    opts->name = "Demeter";
    opts->mem_channel = 4;
    opts->core_mask = "0x4";
    //  struct spdk_pci_addr nic = {0,0x37,0, 0};
    // opts.pci_whitelist = &nic;
    // opts.num_pci_addr = 1;
    // std::string eal_args = "--proc-type=auto";
    // opts.env_context = (void*)eal_args.c_str();

    if (spdk_env_init(&opts) < 0) {
        printf("Unable to initialize SPDK env\n");
        return -1;
    }

    //struct spdk_nvme_transport_id trid;
    //trid.trtype = SPDK_NVME_TRANSPORT_PCIE;
    //sprintf((char*)&trid.traddr, "0000:12:00.0");
    
    if (!parseTransportId(&trid, transportType, devAddress)) {
        return -1;
    }

    if (spdk_nvme_probe(&trid, nullptr, probeCb, attachCb, nullptr) != 0) {
        printf("spdk_nvme_probe failed\n");
        return -1;
    }

    our_spdk_init_flag = true;
    our_init_flag = true;
    return 0;
}

#if DMTR_PROFILE
int dmtr::spdk_queue::alloc_latency()
{
    if (NULL == read_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "read"));
        read_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == write_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "write"));
        write_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    return 0;    
}
#endif

int dmtr::spdk_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = NULL;
    DMTR_TRUE(EPERM, our_spdk_init_flag);

#if DMTR_PROFILE
    DMTR_OK(alloc_latency());
#endif

    q_out = std::unique_ptr<io_queue>(new spdk_queue(qd, FILE_Q));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

int dmtr::spdk_queue::open(const char *pathname, int flags)
{
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    //TODO(ashmrtnz): Yay for only supporing a single file, so we do nothing! If
    // we choose to support multiple files we will need to so some sort of
    // lookup or something here.
    // TODO(ashmrtnz): O_TRUNC?
    start_threads();
    return 0;
}

int dmtr::spdk_queue::open(const char *pathname, int flags, mode_t mode)
{
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    //TODO(ashmrtnz): Yay for only supporing a single file, so we do nothing! If
    // we choose to support multiple files we will need to so some sort of
    // lookup or something here. We can't support O_EXCL right now.
    // TODO(ashmrtnz): O_TRUNC?
    start_threads();
    return 0;
}

int dmtr::spdk_queue::creat(const char *pathname, mode_t mode)
{
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    //TODO(ashmrtnz): Yay for only supporing a single file, so we do nothing! If
    // we choose to support multiple files we will need to so some sort of
    // lookup or something here. We can't support O_EXCL right now.
    // TODO(ashmrtnz): O_TRUNC? Should be implemented if we honor the flag.
    start_threads();
    return 0;
}

int dmtr::spdk_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    DMTR_NOTNULL(EINVAL, my_push_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, sga));
    my_push_thread->enqueue(qt);
    my_push_thread->service();
    return 0;
}

// TODO(ashmrtnz): Update to use spdk scatter gather arrays if the sga parameter
// has DMA-able memory.
int dmtr::spdk_queue::file_push(const dmtr_sgarray_t *sga, task::thread_type::yield_type &yield)
{
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    uint32_t total_len = 0;
    
    // Allocate a DMA-able buffer that is rounded up to the nearest sector size
    // and includes space for the sga metadata like the number of segments, each
    // segment size, and any partial block data from the last write.
    // Randomly pick 4k alignment.
    const unsigned int size = (partialBlockUsage +
        (sga->sga_numsegs * (DMTR_SGARRAY_MAXSIZE + sizeof(uint32_t))) +
        sizeof(uint32_t) + sectorSize - 1) / sectorSize;
    char *payload = (char *) spdk_malloc(size, 0x1000, NULL,
        SPDK_ENV_SOCKET_ID_ANY, SPDK_MALLOC_DMA);
    assert(payload != nullptr);

    // See if we have a partial block left over from the last write. If we do,
    // we need to insert it into the front of the buffer.
    if (partialBlockUsage != 0) {
        memcpy(payload, partialBlock, partialBlockUsage);
        payload += partialBlockUsage;
    }
    
    {
        auto * const u32 = reinterpret_cast<uint32_t *>(payload);
        *u32 = sga->sga_numsegs;
        total_len += sizeof(*u32);
        payload += sizeof(*u32);
    }

    for (size_t i = 0; i < sga->sga_numsegs; i++) {
        auto * const u32 = reinterpret_cast<uint32_t *>(payload);
        const auto len = sga->sga_segs[i].sgaseg_len;
        *u32 = len;
        total_len += sizeof(*u32);
        payload += sizeof(*u32);
        memcpy(payload, sga->sga_segs[i].sgaseg_buf, len);
        total_len += len;
        payload += len;
    }

    // Not sure if this is strictly required or if the device will throw and
    // error all by itself, but just to be safe.
    if (logOffset * sectorSize + total_len > namespaceSize) {
        return -ENOSPC;
    }

    unsigned int numBlocks = total_len / sectorSize;
    partialBlockUsage = total_len - (numBlocks * sectorSize);

    // Save any partial blocks we may have so we don't have to do a
    // read-copy-update on the next write.
    if (partialBlockUsage != 0) {
        memcpy(partialBlock, payload + (total_len - partialBlockUsage),
            partialBlockUsage);
        ++numBlocks;
    }

    int rc = spdk_nvme_ns_cmd_write(ns, qpair, payload, logOffset, numBlocks,
        nullptr, nullptr, 0);
    if (rc != 0) {
        return rc;
    }

    logOffset += numBlocks;
    if (partialBlockUsage != 0) {
        // If we had a partial block, then we're going to rewrite it the next
        // time we get data to write, so go back one LBA.
        --logOffset;
    }

    // Wait for completion.
#if DMTR_PROFILE
    auto t0 = boost::chrono::steady_clock::now();
    boost::chrono::duration<uint64_t, boost::nano> dt(0);
#endif
    do {
        // TODO(ashmrtnz): Assumes that there is only 1 outstanding request at a
        // time, since we're retrieving what we just queued above...
        rc = spdk_nvme_qpair_process_completions(qpair, 1);
        if (rc == 0) {
#if DMTR_PROFILE
            dt += boost::chrono::steady_clock::now() - t0;
#endif
            yield();
#if DMTR_PROFILE
            t0 = boost::chrono::steady_clock::now();
#endif
        }
    } while (rc == 0);
    spdk_free(payload);
    return rc;
}

int dmtr::spdk_queue::push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) 
{
    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));
            
        const dmtr_sgarray_t *sga = NULL;
        DMTR_TRUE(EINVAL, t->arg(sga));

#if DMTR_DEBUG
        std::cerr << "push(" << qt << "): preparing message." << std::endl;
#endif

        size_t sgalen = 0;
        DMTR_OK(dmtr_sgalen(&sgalen, sga));
        if (0 == sgalen) {
            DMTR_OK(t->complete(ENOMSG));
            // move onto the next task.
            continue;
        }

        int ret = 0;
        ret = file_push(sga, yield);

        if (0 != ret) {
            DMTR_OK(t->complete(ret));
            // move onto the next task.
            continue;
        }
        DMTR_OK(t->complete(0, *sga));
    }
    return 0;
}

int dmtr::spdk_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    DMTR_NOTNULL(EINVAL, my_pop_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_POP));
    my_pop_thread->enqueue(qt);

    return 0;
}

int dmtr::spdk_queue::file_pop(dmtr_sgarray_t *sga, task::thread_type::yield_type &yield)
{
    //TODO?
    return 0;
}

int dmtr::spdk_queue::pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
#if DMTR_DEBUG
    std::cerr << "[" << qd() << "] pop thread started." << std::endl;
#endif

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        dmtr_sgarray_t sga = {};
        int ret = 0;
        ret = file_pop(&sga, yield);
        if (EAGAIN == ret) {
            yield();
            continue;
        }

        if (0 != ret) {
            DMTR_OK(t->complete(ret));
            // move onto the next task.
            continue;
        }

        std::cerr << "pop(" << qt << "): sgarray received." << std::endl;
        DMTR_OK(t->complete(0, sga));
    }
    return 0;
}


int dmtr::spdk_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{
    DMTR_OK(task::initialize_result(qr_out, qd(), qt));
    DMTR_TRUE(EPERM, our_spdk_init_flag);
    DMTR_TRUE(EINVAL, good());

    task *t;
    DMTR_OK(get_task(t, qt));

    int ret;
    switch (t->opcode()) {
        default:
            return ENOTSUP;
        case DMTR_OPC_PUSH:
            ret = my_push_thread->service();
            break;
        case DMTR_OPC_POP:
            ret = my_pop_thread->service();
            break;
        case DMTR_OPC_CONNECT:
            ret = 0;
            break;
    }

    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            break;
        case 0:
            // the threads should only exit if the queue has been closed
            // (`good()` => `false`).
            DMTR_UNREACHABLE();
    }

    return t->poll(qr_out);
}


void dmtr::spdk_queue::start_threads() {
    my_push_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield,
                                                   task::thread_type::queue_type &tq) {
            return push_thread(yield, tq);
        }));

    my_pop_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield,
                                                  task::thread_type::queue_type &tq) {
                                                  return pop_thread(yield, tq);
                                              }));
}
