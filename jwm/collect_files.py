#!/usr/bin/env python3
"""
递归收集指定目录下所有文件内容到单个文件
支持：
- 命令行参数指定源目录和输出文件
- 指定若干文件的绝对（或相对）路径进行收集
- 当只指定 -F 时，仅收集这些文件，不递归目录
"""
import os
import argparse
from pathlib import Path
from typing import Iterable, Set, Tuple, List, Optional


def collect_from_directory(
    source_path: Path,
    exclude_dirs: Set[str],
    exclude_files: Set[str]
) -> Tuple[List[str], int, int]:
    """
    从目录递归收集文件内容

    Returns:
        (files_content, file_count, error_count)
    """
    files_content: List[str] = []
    file_count = 0
    error_count = 0

    print(f"源目录: {source_path}")
    print(f"排除目录: {', '.join(sorted(exclude_dirs))}")
    print("-" * 60)

    # 递归遍历所有文件
    for file_path in sorted(source_path.rglob('*')):
        # 跳过目录
        if file_path.is_dir():
            continue

        # 跳过排除的目录中的文件
        if any(excluded in file_path.parts for excluded in exclude_dirs):
            continue

        # 跳过排除的文件
        if file_path.name in exclude_files:
            continue

        # 获取相对路径（相对于 source_path）
        try:
            relative_path = file_path.relative_to(source_path)
        except ValueError:
            relative_path = file_path

        try:
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                content = f.read()

            files_content.append(f"{relative_path}\n{content}")
            file_count += 1
            print(f"✓ 已收集(目录): {relative_path}")
        except Exception as e:
            error_count += 1
            print(f"✗ 跳过(目录): {relative_path} (原因: {e})")

    return files_content, file_count, error_count


def collect_from_file_list(
    files: Iterable[str],
    exclude_dirs: Set[str],
    exclude_files: Set[str],
    source_path: Optional[Path] = None
) -> Tuple[List[str], int, int]:
    """
    从给定文件路径列表中收集内容（支持绝对或相对路径）

    Args:
        files: 文件路径列表
        exclude_dirs: 要排除的目录名称集合
        exclude_files: 要排除的文件名称集合
        source_path: 用于计算相对路径的基准目录（可选）

    Returns:
        (files_content, file_count, error_count)
    """
    files_content: List[str] = []
    file_count = 0
    error_count = 0

    files = list(files)
    if not files:
        return files_content, file_count, error_count

    print("-" * 60)
    print("开始收集指定文件列表:")
    for fp in files:
        file_path = Path(fp).expanduser().resolve()

        # 不存在或非文件则跳过
        if not file_path.exists() or not file_path.is_file():
            error_count += 1
            print(f"✗ 跳过(指定文件): {fp} (不是存在的文件)")
            continue

        # 检查是否属于排除目录（根据路径片段判断）
        if any(excluded in file_path.parts for excluded in exclude_dirs):
            print(f"✗ 跳过(指定文件): {file_path} (所在目录被排除)")
            continue

        # 检查是否在排除文件列表中（按文件名）
        if file_path.name in exclude_files:
            print(f"✗ 跳过(指定文件): {file_path.name} (在排除文件列表中)")
            continue

        # 相对路径显示逻辑：
        # - 如果提供了 source_path 且文件在该目录下，则用相对路径
        # - 否则用绝对路径
        if source_path is not None:
            try:
                relative_path = file_path.relative_to(source_path)
            except ValueError:
                # 不在 source_path 下
                relative_path = file_path
        else:
            relative_path = file_path

        try:
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                content = f.read()

            files_content.append(f"{relative_path}\n{content}")
            file_count += 1
            print(f"✓ 已收集(指定文件): {relative_path}")
        except Exception as e:
            error_count += 1
            print(f"✗ 跳过(指定文件): {relative_path} (原因: {e})")

    return files_content, file_count, error_count


def collect_files(
    source_dir: Optional[str],
    output_file='collected_files.txt',
    exclude_dirs: Optional[Set[str]] = None,
    exclude_files: Optional[Set[str]] = None,
    extra_files: Optional[Iterable[str]] = None,
    enable_dir_scan: bool = True,
):
    """
    统一收集入口：递归目录 + 指定文件列表

    Args:
        source_dir: 源目录路径（如果 enable_dir_scan 为 False，可为 None）
        output_file: 输出文件名
        exclude_dirs: 要排除的目录集合
        exclude_files: 要排除的文件集合
        extra_files: 额外指定的文件路径列表（绝对或相对）
        enable_dir_scan: 是否启用目录递归扫描
    """
    if exclude_dirs is None:
        exclude_dirs = {'.git', '__pycache__', 'node_modules',
                        '.idea', '.vscode', 'target', 'dist', 'build'}

    if exclude_files is None:
        exclude_files = set()

    # 添加输出文件到排除列表（避免把自己收进去）
    exclude_files.add(os.path.basename(output_file))

    source_path: Optional[Path] = None
    if enable_dir_scan:
        if source_dir is None:
            raise ValueError("启用目录扫描时必须提供 source_dir")
        source_path = Path(source_dir).resolve()

        # 检查源目录是否存在
        if not source_path.exists():
            print(f"错误: 目录 '{source_dir}' 不存在")
            return

        if not source_path.is_dir():
            print(f"错误: '{source_dir}' 不是一个目录")
            return

        print(f"源目录: {source_path}")
    else:
        print("未启用目录递归扫描（只收集指定文件列表）")

    print(f"输出文件: {output_file}")
    print(f"排除目录: {', '.join(sorted(exclude_dirs))}")
    print(
        f"排除文件名: {', '.join(sorted(exclude_files)) if exclude_files else '(无)'}")
    print("-" * 60)

    all_contents: List[str] = []
    total_files = 0
    total_errors = 0

    # 1) 目录递归收集（可选）
    if enable_dir_scan and source_path is not None:
        dir_contents, dir_count, dir_errors = collect_from_directory(
            source_path=source_path,
            exclude_dirs=exclude_dirs,
            exclude_files=exclude_files
        )
        all_contents.extend(dir_contents)
        total_files += dir_count
        total_errors += dir_errors

    # 2) 指定文件列表收集（如果有）
    if extra_files:
        extra_contents, extra_count, extra_errors = collect_from_file_list(
            files=extra_files,
            exclude_dirs=exclude_dirs,
            exclude_files=exclude_files,
            source_path=source_path,
        )
        all_contents.extend(extra_contents)
        total_files += extra_count
        total_errors += extra_errors

    # 写入输出文件
    print("-" * 60)
    try:
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write('\n\n'.join(all_contents))

        print(f"✓ 完成! 共收集 {total_files} 个文件")
        if total_errors > 0:
            print(f"✗ 跳过 {total_errors} 个文件（无法读取或不存在）")
        print(f"输出文件: {output_file}")

        # 显示输出文件大小
        file_size = os.path.getsize(output_file)
        if file_size < 1024:
            size_str = f"{file_size} B"
        elif file_size < 1024 * 1024:
            size_str = f"{file_size / 1024:.2f} KB"
        else:
            size_str = f"{file_size / (1024 * 1024):.2f} MB"
        print(f"文件大小: {size_str}")

    except Exception as e:
        print(f"✗ 写入输出文件失败: {e}")


def main():
    parser = argparse.ArgumentParser(
        description='递归收集目录下所有文件内容到单个文件，并可额外指定文件列表',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='''
示例:
  %(prog)s
      # 收集当前目录（默认行为）

  %(prog)s -s /path/to/dir
      # 收集指定目录

  %(prog)s -s ./src -o output.txt
      # 指定输出文件

  %(prog)s -s ./src -e .git target build
      # 排除特定目录

  %(prog)s -s ./src -f a.log b.log
      # 排除特定文件名（按名称精确匹配）

  %(prog)s -F /abs/path/a.py /abs/path/b.txt
      # 只收集这些文件，不递归任何目录

  %(prog)s -s ./src -F ../config.yml /etc/hosts
      # 在收集 src 目录的基础上，再额外收集几个指定文件
        '''
    )

    # 注意：这里不再设置 default='.'，以便区分“是否显式指定 -s”
    parser.add_argument(
        '-s', '--source',
        default=None,
        help='源目录路径；若未指定且未使用 -F，则默认为当前目录'
    )

    parser.add_argument(
        '-o', '--output',
        default='all_files.txt',
        help='输出文件名 (默认: all_files.txt)'
    )

    parser.add_argument(
        '-e', '--exclude-dirs',
        nargs='*',
        default=[],
        help='要排除的目录名称（空格分隔，仅按目录名匹配）'
    )

    parser.add_argument(
        '-f', '--exclude-files',
        nargs='*',
        default=[],
        help='要排除的文件名称（空格分隔，仅按文件名匹配）'
    )

    parser.add_argument(
        '--no-default-excludes',
        action='store_true',
        help='不使用默认排除目录列表'
    )

    parser.add_argument(
        '-F', '--files',
        nargs='*',
        default=[],
        help='额外要收集的文件路径（绝对或相对，空格分隔）'
    )

    args = parser.parse_args()

    # 设置排除目录
    if args.no_default_excludes:
        exclude_dirs = set(args.exclude_dirs)
    else:
        exclude_dirs = {'.git', '__pycache__', 'node_modules', '.vim',
                        '.idea', '.vscode', 'target', 'dist', 'build'}
        exclude_dirs.update(args.exclude_dirs)

    # 设置排除文件
    exclude_files = set(args.exclude_files)

    # 判断是否启用目录扫描：
    # 1) 如果用户显式指定了 -s，则启用目录扫描，source_dir=指定值
    # 2) 如果未指定 -s 且未指定 -F，则默认启用目录扫描，source_dir='.'
    # 3) 如果未指定 -s 且指定了 -F，则仅收集文件列表，不扫描目录
    if args.source is not None:
        # 情况 1：显式指定 -s
        enable_dir_scan = True
        source_dir = args.source
    else:
        if args.files:
            # 情况 3：没有 -s，但是有 -F => 只收集指定文件
            enable_dir_scan = False
            source_dir = None
        else:
            # 情况 2：既没有 -s 也没有 -F => 默认扫描当前目录
            enable_dir_scan = True
            source_dir = '.'

    collect_files(
        source_dir=source_dir,
        output_file=args.output,
        exclude_dirs=exclude_dirs,
        exclude_files=exclude_files,
        extra_files=args.files,
        enable_dir_scan=enable_dir_scan,
    )


if __name__ == '__main__':
    main()

