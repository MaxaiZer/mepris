def "nu-complete mepris steps" [context: string] {
    let args =  ($context | split row -r '\s+')
    mut file = ""
    mut tag = ""

    for i in 0..(($args | length) - 1) {
       let arg = ($args | get $i)

       match $arg {
            "-f" | "--file" => { $file = ($args | get ($i + 1)) }
            "-t" | "--tag"  => { $tag = ($args | get ($i + 1)) }
       }
    }

    if ($file == "") {
        return []
    }

    if ($tag == "") {
        ^mepris list-steps --file $file -p | lines
    } else {
        ^mepris list-steps --file $file --tag $tag -p | lines
    }
}

def "nu-complete mepris tags" [context: string] {
    let args =  ($context | split row -r '\s+')
    mut file = ""

     for i in 0..(($args | length) - 1) {
         let arg = ($args | get $i)

         match $arg {
             "-f" | "--file" => { $file = ($args | get ($i + 1)) }
         }
     }

    if ($file | is-empty) {
        return []
    }

    ^mepris list-tags --file $file | lines
}