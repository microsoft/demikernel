#! /usr/bin/python3

import argparse
import os

def extract_irqs(args):
    irqs = {}
    with open('/proc/interrupts', 'r') as f:
        next(f) #Skip CPU headers line
        for line in f:
            l = line.split()
            irqs[l[-1]] = l[0]

    # We could probably do it in one comprehension, but it becomes really convoluted
    nic_irqs = {k:v.rstrip(':') for k,v in irqs.items() if k.startswith(args.interface)}
    nic_irqs = {k:v for k,v in nic_irqs.items() if int(k.split('-')[1]) in args.queue_ids}

    return nic_irqs

def map_irqs(irqs):
    if not isinstance(irqs, dict):
        return False

    # For each dict item, generate bitmask and write file
    for queue, IRQ in irqs.items():
        cpu = queue.split('-')[1]
        bitmask = '{:x}'.format(2**int(cpu)).zfill(8)
        filename = os.path.join('/proc/irq/', IRQ, 'smp_affinity')
        with open(filename, 'w') as f:
            f.write(bitmask)

def check_mapping(irqs):
    print('Current mapping:')
    for queue, IRQ in irqs.items():
        filename = os.path.join('/proc/irq/', IRQ, 'smp_affinity')
        with open(filename, 'r') as f:
            bitmask = int(f.readline().rstrip('\n'), 16)
        print('IRQ {} (queue {}) can be handled by: {:020b}'.format(
            IRQ, queue, bitmask
        ))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description="Map NIC IRQ to CPUs, one-to-one")
    parser.add_argument('interface', type=str, help='Network interface name')
    parser.add_argument('--queue-ids', '-q', type=int, nargs='*', dest='queue_ids',
                        default=list(range(0,20)), help='NIC queue ID to map')
    parser.add_argument('--list-only', '-l', action='store_true', dest='list_only',
                        help='Only list queue information mapping')

    args = parser.parse_args()

    # Parse /proc/interrupts to extract all the NIC IRQ numbers
    irqs = extract_irqs(args)

    # If list-only is false, proceed to do the one-to-one mapping
    if not args.list_only:
        map_irqs(irqs)
    check_mapping(irqs)
