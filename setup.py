"""PyPI setup for paperless-ngx-cli."""

from setuptools import find_packages, setup

with open("README.md", encoding="utf-8") as f:
    long_description = f.read()

setup(
    name="paperless-ngx-cli",
    version="1.1.0",
    description="Command-line interface for Paperless-ngx document management",
    long_description=long_description,
    long_description_content_type="text/markdown",
    python_requires=">=3.10",
    packages=find_packages(exclude=["*.tests", "*.tests.*"]),
    install_requires=[
        "click>=8.0.0",
        "requests>=2.28.0",
        "prompt_toolkit>=3.0.0",
    ],
    extras_require={
        "dev": [
            "pytest>=7.0.0",
            "pytest-mock>=3.0.0",
            "responses>=0.23.0",
            "ruff>=0.11.0",
            "mypy>=1.15.0",
            "pylint>=3.3.0",
            "bandit>=1.8.0",
            "build>=1.2.0",
            "setuptools>=68",
            "twine>=6.1.0",
            "types-requests>=2.32.0",
            "wheel",
        ]
    },
    entry_points={
        "console_scripts": [
            "paperless=paperless_ngx.paperless_ngx_cli:main",
        ],
    },
    classifiers=[
        "Development Status :: 4 - Beta",
        "Environment :: Console",
        "Intended Audience :: End Users/Desktop",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Programming Language :: Python :: 3.13",
        "Topic :: Utilities",
    ],
    project_urls={
        "Source": "https://github.com/damacus/paperless-cli",
        "Issues": "https://github.com/damacus/paperless-cli/issues",
    },
)
