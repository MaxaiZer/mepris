_mepris_complete_steps() {
    local file tag

    for ((i=1; i<=${#words}; i++)); do
        case "${words[i]}" in
            -f|--file)
                file="${words[i+1]}"
                ;;
            -t|--tag)
                tag="${words[i+1]}"
                ;;
        esac
    done

    if [[ -z "$file" ]]; then
        return 0
    fi

    local -a steps
    if [[ -n "$tag" ]]; then
        steps=( ${(f)"$(mepris list-steps --file "$file" --tag "$tag" --plain 2>/dev/null)"} )
    else
        steps=( ${(f)"$(mepris list-steps --file "$file" --plain 2>/dev/null)"} )
    fi

    _describe 'steps' steps
}

_mepris_complete_tags() {
    local file
    for ((i=1; i<=${#words}; i++)); do
        case "${words[i]}" in
            -f|--file)
                file="${words[i+1]}"
                ;;
        esac
    done

    if [[ -z "$file" ]]; then
        return 0
    fi

    local -a tags
    tags=( ${(f)"$(mepris list-tags --file "$file" 2>/dev/null)"} )
    _describe 'tags' tags
}