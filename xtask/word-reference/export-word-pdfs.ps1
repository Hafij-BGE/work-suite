<#
.SYNOPSIS
  Exports every .docx in a folder to PDF with Microsoft Word, for `cargo xtask fidelity`.

.DESCRIPTION
  Opens each document read-only through Word's COM automation, exports it with
  ExportAsFixedFormat (PDF, optimised for print, whole document), closes it without saving,
  and writes manifest.json with Word's version and build, the active printer, Windows' version
  and which of the corpus fonts Word has. The PDFs are Word's output: keep them local, never
  commit them (docs/word-fidelity.md).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File export-word-pdfs.ps1 -Corpus C:\wordcraft-fidelity\corpus
#>
param(
  [Parameter(Mandatory = $true)][string]$Corpus,
  [string]$Out = ""
)
$ErrorActionPreference = "Stop"
$Corpus = (Resolve-Path $Corpus).Path
if ($Out -eq "") { $Out = Join-Path $Corpus "pdf" }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0   # wdAlertsNone
$results = @()
try {
  $fonts = @($word.FontNames)
  foreach ($f in Get-ChildItem -Path $Corpus -Filter *.docx | Sort-Object Name) {
    if ($f.Name.StartsWith("~$")) { continue }
    $pdf = Join-Path $Out ($f.BaseName + ".pdf")
    $row = [ordered]@{ docx = $f.Name; pdf = [IO.Path]::GetFileName($pdf) }
    $doc = $null
    try {
      # Open(FileName, ConfirmConversions, ReadOnly, AddToRecentFiles)
      $doc = $word.Documents.Open($f.FullName, $false, $true, $false)
      $doc.Repaginate()
      $row.pages = $doc.ComputeStatistics(2)   # wdStatisticPages
      $row.compatibility_mode = $doc.CompatibilityMode
      # ExportAsFixedFormat(OutputFileName, wdExportFormatPDF=17, OpenAfterExport, wdExportOptimizeForPrint=0)
      $doc.ExportAsFixedFormat($pdf, 17, $false, 0)
      $row.ok = $true
    } catch {
      $row.ok = $false
      $row.error = $_.Exception.Message
    } finally {
      if ($doc -ne $null) { $doc.Close(0) }   # wdDoNotSaveChanges
    }
    Write-Host ("{0} -> {1} pages {2}" -f $f.Name, $row.pdf, $row.pages)
    $results += [pscustomobject]$row
  }
  $want = "Aptos", "Calibri", "Cambria", "Times New Roman", "Arial", "Courier New"
  $manifest = [ordered]@{
    word_version   = $word.Version
    word_build     = $word.Build
    active_printer = $word.ActivePrinter
    windows        = [Environment]::OSVersion.VersionString
    exported_at    = (Get-Date).ToString("o")
    fonts_present  = [ordered]@{}
    documents      = $results
  }
  foreach ($n in $want) { $manifest.fonts_present[$n] = $fonts -contains $n }
  $manifest | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $Out "manifest.json")
} finally {
  $word.Quit(0)
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
Write-Host "Done: $($results.Count) documents, PDFs in $Out"
