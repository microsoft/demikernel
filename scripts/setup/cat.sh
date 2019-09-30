#!/bin/bash -xe


# This script takes:
# - the cores we want to CAT
# - the number of ways we want to give to each core
# And will exclude those ways from the default way mask,
# And set the cores to use the CAT'ed way masks
# See: https://www.intel.com/content/dam/www/public/us/en/documents/white-papers/cache-allocation-technology-white-paper.pdf
# And 17.19 of the Intel manual

num_cpus=9 # starts at 0
cated_cores=(0)
uncated_cores=(1 2 3 4 5 6 7 8 9)
wpc=2
reset=$1

base_reg=0xc90 # This holds the default way mask
IA32_PQR_ASSOC=0xc8f
ways=11
default_mask=$((2**$ways - 1))
way_masks=8 # it's 9, but the bitmask is then 16**8

if [ $reset == "reset" ]; then # FIXME: first test if $1 is set
    # Reset the base register
    wrmsr -a $base_reg $default_mask
    n=$(($way_masks - 1))
    for i in $(seq 1 $n); do
        reg=$(($base_reg + $i))
        wrmsr $reg $default_mask # Reset all the way masks
    done

    # Set all CPU to use the default way mask
    wrmsr -a $IA32_PQR_ASSOC 0
else
    uncated_ways=$((ways - wpc*${#cated_cores[*]}))
    new_default_mask=$(((2**$uncated_ways - 1 )))
    #new_default_mask=$((2**$ways - 1))

    # Update the base register to exclude those ways we want to allocate
    wrmsr -a $base_reg $new_default_mask

    for i in ${!cated_cores[*]}; do
        core_mask=0
        n=$(($wpc - 1))
        for w in $(seq 0 $n); do
            core_mask=$(($core_mask + 2**(i*$wpc + $w)))
        done

        reg=$(($base_reg + $i + 1)) #set way mask
        bm=$((16**($way_masks - $i)))
        wm=`printf "0x%x" $bm`
        wrmsr -p ${cated_cores[$i]} $reg $core_mask  # set COS
        wrmsr -p ${cated_cores[$i]} $IA32_PQR_ASSOC $wm # associate CPU with COS
    done

    # Set all other mask registers to default mask (should be useless)
    #offset=$((${#cated_cores[*]} + 1))
    #for i in $(seq $offset $way_masks); do
    #    reg=$(($base_reg + $i))
    #    wrmsr -a $reg $default_mask
    #done

    # Have all other cores use the default way mask (should be useless)
    #for core in ${uncated_cores[*]}; do
    #    wrmsr -p ${uncated_cores[$i]} 0xc8f 0
    #done
fi
# Other resources for CAT stuff:
# MBA MSR starts at address 0xD50
