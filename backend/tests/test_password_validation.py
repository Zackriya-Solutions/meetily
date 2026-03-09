"""Tests for password complexity validation."""

import pytest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

from auth_models import _check_password_complexity


def test_valid_password():
    assert _check_password_complexity("TestPass1") == "TestPass1"


def test_too_short():
    with pytest.raises(ValueError, match="at least 8 characters"):
        _check_password_complexity("Short1")


def test_no_uppercase():
    with pytest.raises(ValueError, match="uppercase"):
        _check_password_complexity("testpass1")


def test_no_lowercase():
    with pytest.raises(ValueError, match="lowercase"):
        _check_password_complexity("TESTPASS1")


def test_no_digit():
    with pytest.raises(ValueError, match="digit"):
        _check_password_complexity("TestPasss")


def test_minimum_valid():
    assert _check_password_complexity("Abcdefg1") == "Abcdefg1"
