/*
 * lwip-queue.h
 *
 *  Created on: Jun 11, 2018
 *      Author: amanda
 */

#ifndef LIBOS_LIBLWIP_LWIP_QUEUE_H_
#define LIBOS_LIBLWIP_LWIP_QUEUE_H_

#include "include/io-queue.h"
#include "common/queue.h"
#include <list>
#include <map>

namespace Zeus {
namespace LWIP {

class LWIPQueue : public Zeus::Queue {
public:
    bool is_bound = false;
    sockaddr_in bound_addr;
};

} // namespace LWIP
} // namespace Zeus



#endif /* LIBOS_LIBLWIP_LWIP_QUEUE_H_ */
