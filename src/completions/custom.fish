function __fish_mepris_step
    set -l file (__fish_get_file_arg)
    set -l tags (__fish_get_arg_value -t --tag)

    if test -n "$file" -a -f "$file"
        if test -n "$tags"
            mepris list-steps -f $file -t $tags -p 2>/dev/null
        else
            mepris list-steps -f $file -p 2>/dev/null
        end
    end
end

function __fish_mepris_tag
    set -l file (__fish_get_file_arg)
    set -l file (eval echo $file)

    if test -n "$file" -a -f "$file"
        mepris list-tags -f $file 2>/dev/null
    end
end

function __fish_get_file_arg
    set -l file (__fish_get_arg_value -f --file)
    echo (eval echo $file) #convert ~/... to valid path
end

function __fish_get_arg_value
    set -l tokens (commandline -opc)
    set -l short_flag $argv[1]
    set -l long_flag $argv[2]

    for i in (seq (count $tokens))
        if test $tokens[$i] = $short_flag
            echo $tokens[(math $i + 1)]
            return
        else if test $tokens[$i] = $long_flag
            echo $tokens[(math $i + 1)]
            return
        end
    end
end

complete -c mepris -s s -l step -a "(__fish_mepris_step)" --no-files -r
complete -c mepris -s t -l tag -a "(__fish_mepris_tag)" --no-files -r
