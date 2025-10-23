#!/usr/bin/env python3
"""
递归收集指定目录下所有文件内容到单个文件
支持命令行参数指定源目录和输出文件
"""
import os
import argparse
from pathlib import Path


def collect_files(source_dir='.', output_file='collected_files.txt', exclude_dirs=None, exclude_files=None):
    """
    递归收集所有文件内容

    Args:
        source_dir: 源目录路径
        output_file: 输出文件名
        exclude_dirs: 要排除的目录集合
        exclude_files: 要排除的文件集合
    """
    if exclude_dirs is None:
        exclude_dirs = {'.git', '__pycache__', 'node_modules',
                        '.idea', '.vscode', 'target', 'dist', 'build'}

    if exclude_files is None:
        exclude_files = set()

    # 添加输出文件到排除列表
    exclude_files.add(os.path.basename(output_file))

    source_path = Path(source_dir).resolve()

    # 检查源目录是否存在
    if not source_path.exists():
        print(f"错误: 目录 '{source_dir}' 不存在")
        return

    if not source_path.is_dir():
        print(f"错误: '{source_dir}' 不是一个目录")
        return

    print(f"源目录: {source_path}")
    print(f"输出文件: {output_file}")
    print(f"排除目录: {', '.join(sorted(exclude_dirs))}")
    print("-" * 60)

    files_content = []
    file_count = 0
    error_count = 0

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

        # 获取相对路径
        try:
            relative_path = file_path.relative_to(source_path)
        except ValueError:
            # 如果无法获取相对路径，使用绝对路径
            relative_path = file_path

        try:
            # 尝试读取文件内容（文本文件）
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                content = f.read()

            # 格式化：相对路径 + 换行 + 内容
            files_content.append(f"{relative_path}\n{content}")
            file_count += 1
            print(f"✓ 已收集: {relative_path}")

        except Exception as e:
            error_count += 1
            print(f"✗ 跳过: {relative_path} (原因: {e})")

    # 写入输出文件
    try:
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write('\n\n'.join(files_content))

        print("-" * 60)
        print(f"✓ 完成! 共收集 {file_count} 个文件")
        if error_count > 0:
            print(f"✗ 跳过 {error_count} 个文件（无法读取）")
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
        description='递归收集目录下所有文件内容到单个文件',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='''
示例:
  %(prog)s                                    # 收集当前目录
  %(prog)s -s /path/to/dir                   # 收集指定目录
  %(prog)s -s ./src -o output.txt            # 指定输出文件
  %(prog)s -s ./src -e .git target build     # 排除特定目录
  %(prog)s -s ./src -f "*.pyc" "*.log"       # 排除特定文件
        '''
    )

    parser.add_argument(
        '-s', '--source',
        default='.',
        help='源目录路径 (默认: 当前目录)'
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
        help='要排除的目录名称（空格分隔）'
    )

    parser.add_argument(
        '-f', '--exclude-files',
        nargs='*',
        default=[],
        help='要排除的文件名称（空格分隔）'
    )

    parser.add_argument(
        '--no-default-excludes',
        action='store_true',
        help='不使用默认排除目录列表'
    )

    args = parser.parse_args()

    # 设置排除目录
    if args.no_default_excludes:
        exclude_dirs = set(args.exclude_dirs)
    else:
        exclude_dirs = {'.git', '__pycache__', 'node_modules', ".vim",
                        '.idea', '.vscode', 'target', 'dist', 'build'}
        exclude_dirs.update(args.exclude_dirs)

    # 设置排除文件
    exclude_files = set(args.exclude_files)

    # 执行收集
    collect_files(
        source_dir=args.source,
        output_file=args.output,
        exclude_dirs=exclude_dirs,
        exclude_files=exclude_files
    )


if __name__ == '__main__':
    main()
