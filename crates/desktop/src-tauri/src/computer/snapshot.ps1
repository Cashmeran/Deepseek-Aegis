Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = [System.Windows.Automation.Condition]::TrueCondition
$scope = [System.Windows.Automation.TreeScope]::Descendants
$results = @()
$c = 0
$max = 80

# FindAll is much faster and more reliable than manual tree walking
$elements = $root.FindAll($scope, $cond)
foreach ($el in $elements) {
  if ($c -ge $max) { break }
  try {
    $ctrl = $el.Current
    $name = $ctrl.Name
    if (-not $name) { continue }
    $type = $ctrl.ControlType.ProgrammaticName -replace 'ControlType\.',''
    $skip = @('Text','Group','Pane','Window','TitleBar','ScrollBar','Thumb','Header','ToolBar','MenuBar','StatusBar','SplitButton','Separator','AppBar','Custom','Table','Tree','TreeItem','DataGrid','DataItem','SemanticZoom')
    if ($type -in $skip) { continue }
    if (-not $ctrl.IsEnabled) { continue }
    if ($ctrl.IsOffscreen) { continue }
    $r = $ctrl.BoundingRectangle
    if ($r.Width -le 0 -or $r.Height -le 0) { continue }
    $results += [PSCustomObject]@{
      l = $c
      name = $name
      type = $type
      x = [int]$r.X
      y = [int]$r.Y
      w = [int]$r.Width
      h = [int]$r.Height
    }
    $c++
  } catch {}
}
$results | ConvertTo-Json -Depth 3 -Compress
