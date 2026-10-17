"""演示模块 1：故意平铺在根目录，用来触发 no-src-test-layout。

判据：根附近看不到 src / tests 这类划分，而源码文件 >= 8 个。
"""


def step_1(value: int) -> int:
    """第 1 步。"""
    return value + 1
