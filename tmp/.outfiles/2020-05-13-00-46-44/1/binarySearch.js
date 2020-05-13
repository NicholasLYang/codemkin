function binarySearch(list, value, left = 0, right = list.length - 1) {
    if (value > list[right] || value < list[left]) {
	return -1;
    }
    const mid = Math.floor((left + right)/2);
    if (value === list[mid]) {
	return mid;
    }
    if (mid == left || mid == right) {
	return -1;
    }
    if (list[mid] > value) {
	return binarySearch(list, value, left, mid);
    } else {
	return binarySearch(list, value, mid, right);
    }
}

console.log(binarySearch([1, 2, 3, 4, 5, 6, 7], 6));
