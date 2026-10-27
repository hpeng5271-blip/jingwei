# Control-group CI: three checks (raw/edge/dedup) against independent B.
# NOTE: B shares the tree-sitter *grammar* with jw; traversal/aggregation is independent.
# 1) raw == edge + dedup (definition sanity)
# 2) edge_distinct(src,leaf,line) == B_distinct  (real independent check)
# 3) dedup == B_dup - (edge - edge_distinct)     (equal to B_raw == jw_raw given 2)
param(
    [string]$Repo = "D:\projects\repos\rg"
)
$py = "C:\Users\cjz\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe"
& $py "D:\projects\jingwei\.pro\exp\b50_ci_checks.py" $Repo
