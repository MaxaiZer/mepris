if [[ "$prev" == "--step" || "$prev" == "-s" ]]; then
    file=""
    tag=""

    for ((i=0; i<${#COMP_WORDS[@]}; i++)); do
        case "${COMP_WORDS[i]}" in
            --file|-f)
                file="${COMP_WORDS[i+1]}"
                ;;
            --tag|-t)
                tag="${COMP_WORDS[i+1]}"
                ;;
        esac
    done

    if [[ -n "$file" ]]; then
        if [[ -n "$tag" ]]; then
            COMPREPLY=( $(mepris list-steps --file "$file" --tag "$tag" -p 2>/dev/null) )
        else
            COMPREPLY=( $(mepris list-steps --file "$file" -p 2>/dev/null) )
        fi
        return 0
    fi
fi

if [[ "$prev" == "--tag" || "$prev" == "-t" ]]; then
    file=""
    for ((i=0; i<${#COMP_WORDS[@]}; i++)); do
        case "${COMP_WORDS[i]}" in
            --file|-f)
                file="${COMP_WORDS[i+1]}"
                ;;
        esac
    done

    if [[ -n "$file" ]]; then
        COMPREPLY=( $(mepris list-tags --file "$file" 2>/dev/null) )
        return 0
    fi
fi