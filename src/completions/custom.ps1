Register-ArgumentCompleter -CommandName 'mepris' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $results = & $script:oldCompleter $wordToComplete $commandAst $cursorPosition

    $elements = $commandAst.CommandElements | ForEach-Object { $_.ToString() }

    $fileIndex = ($elements | Where-Object { $_ -match '^-f$|^--file$' })
    if ($fileIndex) {
        $filePath = $elements[$elements.IndexOf($fileIndex) + 1]
    }

    $tagIndex = ($elements | Where-Object { $_ -match '^-t$|^--tag$' })
    if ($tagIndex) {
        $tagValueIndex = $elements.IndexOf($tagIndex)
        if ($tagValueIndex -ge 0 -and ($tagValueIndex + 1) -lt $elements.Count) {
            $tagValue = $elements[$tagValueIndex + 1]
        }
    }

    $dynamicResults = @()

    if ($elements -contains '-s' -or $elements -contains '--step') {
        if ($filePath) {
            if ($tagValue) {
                $output = & mepris list-steps -f $filePath -t $tagValue --plain 2>$null
            } else {
                $output = & mepris list-steps -f $filePath --plain 2>$null
            }

            $dynamicResults = $output | ForEach-Object {
                [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterValue, $_)
            }
        }
    }

    elseif ($elements -contains '-t' -or $elements -contains '--tag') {
        if ($filePath) {
            $output = & mepris list-tags -f $filePath 2>$null
            $dynamicResults = $output | ForEach-Object {
                [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterValue, $_)
            }
        }
    }

   if ($elements[-1] -in @('-f', '--file')) {
      $dynamicResults = Get-ChildItem -File | ForEach-Object {
          [CompletionResult]::new($_.Name, $_.Name, [CompletionResultType]::ParameterValue, $_.FullName)
      }
   }

   $res = if ($dynamicResults) {
        $dynamicResults
    } else {
        $results
    }
    $res | Where-Object { $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
