#!/bin/bash -xe

# This script takes:
# - the cores we want to CAT
# - the number of ways we want to give to each core
# And will exclude those ways from the default way mask,
# And set the cores to use the CAT'ed way masks
# See: https://www.intel.com/content/dam/www/public/us/en/documents/white-papers/cache-allocation-technology-white-paper.pdf

num_cpus=9 # starts at 0
cated_cores=(2 3)
uncated_cores=(1 4 5 6 7 8 9)
wpc=1

base_reg=0xc90 # This holds the default way mask
ways=11
let "ways_b = 2**$ways - 1"
default_mask=`printf '0x%x' $ways_b`
way_masks=16 # Manually found

if [ $1 == "reset" ]; then # FIXME: first test if $1 is set
    # Reset the base register
    wrmsr $base_reg $default_mask
    let "n = $way_masks - 1"
    for i in $(seq 1 $n); do
        let "reg = $base_reg + $i"
        wrmsr $reg $default_mask # Reset all the way masks
    done

    # Set all CPU to use the default way mask
    for i in $(seq 0 $num_cpus); do
        wrmsr -p $i 0xc8 0
    done
else
    let "n_ways_b = $ways_b - (2**($wpc*${#cated_cores[*]}) - 1)"
    new_default_mask=`printf '0x%x' $n_ways_b`

    # Update the base register to exclude those ways we want to allocate
    base_reg=0xc90 # This holds the default way mask
    wrmsr $base_reg $new_default_mask

    for i in ${!cated_cores[*]}; do
        let "wm = $i + 1"

        core_mask=0
        n=$(($wpc - 1))
        for w in $(seq 0 $n); do
            let "core_mask = $core_mask + 2**(i*$wpc + $w)"
        done

        let "reg = $base_reg + $wm" #set way mask
        wrmsr $reg $core_mask
        wrmsr -p ${cated_cores[$i]} 0xc8 $wm # have cpu use way mask
    done

    # Have all other cores use the default way mask (should be useless)
    for core in ${uncated_cores[*]}; do
        wrmsr -p ${uncated_cores[$i]} 0xc8 0
    done
fi
# Other resources for CAT stuff:
# MBA MSR starts at address 0xD50
