; Calculate factorial by recursion

(defun fact (n)
  (if (<= n 1)
      1
      (* n (fact (- n 1)))))

(begin
  (print (fact 6))
  (halt))
