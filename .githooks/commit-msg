#!/bin/sh

commit_msg_file=$1
commit_msg=`cat $commit_msg_file`
max_length=50

if [ $(echo "$commit_msg_title" | wc -c) -gt $max_length ]; then
    echo "Commit message title is too long (maximum $max_length characters)"
    exit 1
fi

if ! echo "$commit_msg" | grep -q -E 'Merge pull request|Merge branch|\[[[:alnum:]-]+\] (Workaround|Enhancement|Feature|Bug Fix): '; then
	echo "Commit message does not match pattern: '[module-name] (Workaround|Enhancement|Feature|Bug Fix): Short Description"
    exit 1
fi
